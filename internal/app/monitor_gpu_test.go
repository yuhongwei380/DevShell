package app

import (
	"strings"
	"testing"
	"time"
)

func TestParseMonitorGPUsNVIDIACSV(t *testing.T) {
	gpus := parseMonitorGPUs([]string{
		"0, NVIDIA GeForce RTX 4090, 1234, 24576, 45",
		"1, NVIDIA H100 PCIe, 0, 81559, 0",
		"2, NVIDIA A100-PCIE-40GB, [Not Supported], [Not Supported], [Not Supported]",
		"3, Some, Comma, Name, GPU, 512, 1024, 10",
	})
	if len(gpus) != 3 {
		t.Fatalf("expected 3 usable GPUs, got %d: %+v", len(gpus), gpus)
	}
	if gpus[0].Name != "NVIDIA GeForce RTX 4090" || gpus[0].MemUsed != 1234<<20 || gpus[0].MemTotal != 24576<<20 || gpus[0].Util != 45 || gpus[0].Index != 0 {
		t.Fatalf("unexpected first GPU: %+v", gpus[0])
	}
	if gpus[1].Name != "NVIDIA H100 PCIe" || gpus[1].MemTotal != 81559<<20 || gpus[1].Index != 1 {
		t.Fatalf("unexpected second GPU: %+v", gpus[1])
	}
	// Names containing commas keep everything between the index and the last
	// three numeric columns as the display name.
	if gpus[2].Name != "Some, Comma, Name, GPU" || gpus[2].MemUsed != 512<<20 || gpus[2].MemTotal != 1024<<20 || gpus[2].Util != 10 || gpus[2].Index != 3 {
		t.Fatalf("unexpected comma-named GPU: %+v", gpus[2])
	}
}

// A card that drops off the bus leaves a gap in nvidia-smi's index column;
// the survivors must keep their real ordinals instead of being renumbered.
func TestParseMonitorGPUsKeepsNVIDIAIndexAfterDropout(t *testing.T) {
	gpus := parseMonitorGPUs([]string{
		"1, NVIDIA GeForce RTX 4090, 1234, 24576, 45",
		"2, NVIDIA GeForce RTX 4090, 0, 24576, 0",
		"3, NVIDIA GeForce RTX 4090, 10, 24576, 5",
	})
	if len(gpus) != 3 {
		t.Fatalf("expected 3 surviving GPUs, got %+v", gpus)
	}
	for i, expected := range []int{1, 2, 3} {
		if gpus[i].Index != expected {
			t.Fatalf("GPU after dropout must keep nvidia-smi index %d, got %+v", expected, gpus[i])
		}
	}
}

func TestParseMonitorGPUsAMDSysfs(t *testing.T) {
	gpus := parseMonitorGPUs([]string{
		"amd\t/sys/class/drm/card2\t2147483648\t25769803776\t18",
		"amd\t/sys/class/drm/card1\t0\t0\t0",
		"amd\t/sys/class/drm/card3\tbogus",
	})
	if len(gpus) != 1 {
		t.Fatalf("expected only the discrete AMD card, got %+v", gpus)
	}
	gpu := gpus[0]
	if gpu.Name != "AMD 独立显卡 (card2)" || gpu.MemUsed != 2147483648 || gpu.MemTotal != 25769803776 || gpu.Util != 18 || gpu.Index != 0 {
		t.Fatalf("unexpected AMD GPU: %+v", gpu)
	}
}

func TestParseMonitorGPUsEmptyAndNoise(t *testing.T) {
	for _, lines := range [][]string{nil, {}, {"not a gpu row"}, {"0, OnlyName"}} {
		if gpus := parseMonitorGPUs(lines); len(gpus) != 0 {
			t.Fatalf("expected no GPUs for %q, got %+v", lines, gpus)
		}
	}
}

func TestStatsCarriesGPUsAndStaticRefreshKeepsThem(t *testing.T) {
	fixture := "__CS_STAT__\ncpu 100 0 0 100 0 0 0 0\n__CS_MEM__\nMemTotal: 1024 kB\n__CS_GPU__\n0, NVIDIA GeForce RTX 4090, 1234, 24576, 45\n"
	r, err := parseStats([]byte(fixture), time.Now())
	if err != nil {
		t.Fatal(err)
	}
	if len(r.GPUs) != 1 || r.GPUs[0].Name != "NVIDIA GeForce RTX 4090" {
		t.Fatalf("parseStats lost GPUs: %+v", r.GPUs)
	}
	// A core-only sample without the GPU section inherits the last static list.
	previous := r.Stats
	core, err := parseStats([]byte("__CS_STAT__\ncpu 100 0 0 100 0 0 0 0\n__CS_MEM__\nMemTotal: 1024 kB\n"), time.Now())
	if err != nil {
		t.Fatal(err)
	}
	if len(core.GPUs) != 0 {
		t.Fatal("core sample should not invent GPUs")
	}
	applyStaticMetadata(&core, &previous)
	if len(core.GPUs) != 1 || core.GPUs[0].MemTotal != 24576<<20 {
		t.Fatalf("static metadata merge dropped GPUs: %+v", core.GPUs)
	}
}

func TestGPUCollectionOnlyRunsOnStaticSamples(t *testing.T) {
	if !strings.Contains(monitorCommandFor(true, false), "__CS_GPU__") {
		t.Fatal("static sample must collect GPUs")
	}
	if strings.Contains(monitorCommandFor(false, false), "__CS_GPU__") {
		t.Fatal("lightweight sample must not run GPU probes")
	}
	for _, required := range []string{"nvidia-smi", "mem_info_vram_total", "gpu_busy_percent"} {
		if !strings.Contains(monitorCommandFor(true, false), required) {
			t.Fatal("GPU probe missing", required)
		}
	}
}
