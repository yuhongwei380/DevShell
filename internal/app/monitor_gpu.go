package app

import (
	"fmt"
	"math"
	"strconv"
	"strings"
)

// parseMonitorGPUs turns the __CS_GPU__ section into discrete GPU entries.
// NVIDIA rows arrive as nvidia-smi CSV (index, name, memory.used MiB,
// memory.total MiB, utilization %); AMD rows arrive as tab-separated
// "amd<TAB>card path<TAB>vram used bytes<TAB>vram total bytes<TAB>busy %".
// The sampling shell already skips APUs and integrated GPUs (no dedicated
// VRAM), and the parser re-checks totals so malformed rows are dropped.
func parseMonitorGPUs(lines []string) []GPU {
	gpus := []GPU{}
	for _, line := range lines {
		if strings.HasPrefix(line, "amd\t") {
			fields := strings.Split(line, "\t")
			if len(fields) != 5 {
				continue
			}
			total := number(fields[3])
			if total == 0 {
				continue
			}
			card := fields[1]
			if slash := strings.LastIndex(card, "/"); slash >= 0 {
				card = card[slash+1:]
			}
			name := "AMD 独立显卡"
			if card != "" && card != "device" {
				name = fmt.Sprintf("AMD 独立显卡 (%s)", card)
			}
			gpus = append(gpus, GPU{Index: len(gpus), Name: name, MemUsed: number(fields[2]), MemTotal: total, Util: percentValue(fields[4])})
			continue
		}
		fields := strings.Split(line, ",")
		if len(fields) < 5 {
			continue
		}
		// Keep the nvidia-smi ordinal: after a card drops off the bus the probe
		// still reports the surviving GPUs with their real indices, and the
		// panel must not renumber them into the missing card's slot.
		index := len(gpus)
		if parsed, err := strconv.Atoi(strings.TrimSpace(fields[0])); err == nil {
			index = parsed
		}
		util := percentValue(fields[len(fields)-1])
		total := decimal(strings.TrimSpace(fields[len(fields)-2]))
		used := decimal(strings.TrimSpace(fields[len(fields)-3]))
		name := strings.TrimSpace(strings.Join(fields[1:len(fields)-3], ","))
		if name == "" || total <= 0 || used < 0 {
			continue
		}
		gpus = append(gpus, GPU{Index: index, Name: name, MemUsed: uint64(used) << 20, MemTotal: uint64(total) << 20, Util: util})
	}
	return gpus
}

// percentValue parses a 0-100 utilization figure; "[N/A]" and noise read as 0.
func percentValue(raw string) float64 {
	value := decimal(strings.TrimSpace(raw))
	if math.IsNaN(value) || math.IsInf(value, 0) || value < 0 {
		return 0
	}
	return min(100, value)
}
