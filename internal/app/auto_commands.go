package app

import (
	"errors"
	"strings"
)

// Connection profiles may carry commands typed into the session terminal
// after the remote shell reports ready (issue #5). The web client owns the
// actual typing; the store only persists and validates the configuration.

const (
	maxAutoCommands   = 50
	maxAutoCommandLen = 2000
)

// normalizeAutoCommands cleans the stored command list in place and reports a
// user-facing error only for limits a client can act on. Escape and NUL bytes
// are stripped because a stored list must not be able to forge terminal
// control sequences once replayed into a live session. Trailing CR is dropped
// because every command is submitted separately.
func normalizeAutoCommands(p *Profile) error {
	if p.Protocol == "serial" || p.Protocol == "rdp" {
		p.AutoCommands = nil
		p.AutoRun = false
		return nil
	}
	if p.AutoCommands == nil {
		return nil
	}
	cleaned := make([]string, 0, len(p.AutoCommands))
	for _, line := range p.AutoCommands {
		line = strings.ReplaceAll(line, "\x00", "")
		line = strings.ReplaceAll(line, "\x1b", "")
		line = strings.ReplaceAll(line, "\r", "")
		if strings.TrimSpace(line) == "" {
			continue
		}
		if len(line) > maxAutoCommandLen {
			return errors.New("连接后自动执行命令过长")
		}
		cleaned = append(cleaned, line)
	}
	if len(cleaned) > maxAutoCommands {
		return errors.New("连接后自动执行命令最多 50 行")
	}
	// Keep an explicit empty list non-nil: the edit-merge in Store.saveProfile
	// treats nil as "editing client omitted the field", while an empty slice
	// means "clear the stored commands". Collapsing to nil here would let
	// stored lines survive a deliberate clear.
	p.AutoCommands = cleaned
	if len(cleaned) == 0 {
		p.AutoRun = false
	}
	return nil
}
