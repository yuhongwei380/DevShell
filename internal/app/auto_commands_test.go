package app

import (
	"path/filepath"
	"strings"
	"testing"
)

func TestProfileAutoCommandsPersistAndClear(t *testing.T) {
	a, err := New(filepath.Join(t.TempDir(), "config"))
	if err != nil {
		t.Fatal(err)
	}
	defer a.Close()
	saved, err := a.store.Save(Profile{Name: "auto", Host: "example.com", Port: 22, User: "u", Auth: "password", AutoRun: true, AutoCommands: []string{"cd /data\r", "tmux attach -t dev || tmux new -s dev", "   ", "\x1b", "clear\x00"}}, false)
	if err != nil {
		t.Fatal(err)
	}
	if !saved.AutoRun || len(saved.AutoCommands) != 3 || saved.AutoCommands[0] != "cd /data" || saved.AutoCommands[1] != "tmux attach -t dev || tmux new -s dev" || saved.AutoCommands[2] != "clear" {
		t.Fatalf("stored auto commands = run=%v %+v", saved.AutoRun, saved.AutoCommands)
	}
	// An editing client that omits the field keeps the stored lines and flag.
	if _, err = a.store.Save(Profile{ID: saved.ID, Name: "renamed", Host: "example.com", Port: 22, User: "u", Auth: "password"}, false); err != nil {
		t.Fatal(err)
	}
	stored, err := a.store.Get(saved.ID)
	if err != nil {
		t.Fatal(err)
	}
	if !stored.AutoRun || len(stored.AutoCommands) != 3 {
		t.Fatalf("editing dropped auto commands: run=%v %+v", stored.AutoRun, stored.AutoCommands)
	}
	// An explicit empty list clears both the lines and the run flag.
	if _, err = a.store.Save(Profile{ID: saved.ID, Name: "renamed", Host: "example.com", Port: 22, User: "u", Auth: "password", AutoCommands: []string{}}, false); err != nil {
		t.Fatal(err)
	}
	stored, _ = a.store.Get(saved.ID)
	if stored.AutoRun || len(stored.AutoCommands) != 0 {
		t.Fatalf("explicit empty list did not clear: run=%v %+v", stored.AutoRun, stored.AutoCommands)
	}
	// Limits a client can act on.
	tooMany := make([]string, maxAutoCommands+1)
	for i := range tooMany {
		tooMany[i] = "echo hi"
	}
	if _, err = a.store.Save(Profile{ID: saved.ID, Name: "bad", Host: "example.com", Port: 22, User: "u", Auth: "password", AutoCommands: tooMany}, false); err == nil {
		t.Fatal("line count above the cap was accepted")
	}
	if _, err = a.store.Save(Profile{ID: saved.ID, Name: "bad", Host: "example.com", Port: 22, User: "u", Auth: "password", AutoCommands: []string{strings.Repeat("x", maxAutoCommandLen+1)}}, false); err == nil {
		t.Fatal("overlong command line was accepted")
	}
	// Turning the flag off keeps the lines for later use.
	if _, err = a.store.Save(Profile{ID: saved.ID, Name: "renamed", Host: "example.com", Port: 22, User: "u", Auth: "password", AutoRun: false, AutoCommands: []string{"uptime"}}, false); err != nil {
		t.Fatal(err)
	}
	stored, _ = a.store.Get(saved.ID)
	if stored.AutoRun || len(stored.AutoCommands) != 1 {
		t.Fatalf("disabled flag lost the lines: run=%v %+v", stored.AutoRun, stored.AutoCommands)
	}
}
