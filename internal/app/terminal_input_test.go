package app

import (
	"bytes"
	"encoding/base64"
	"encoding/json"
	"testing"
)

func TestNativeTerminalInputPreservesVTBytes(t *testing.T) {
	raw := []byte{0x1b, '[', 'M', 0x20, 0xff, 0x80}
	encoded, _ := json.Marshal(map[string]any{"type": "input", "dataBase64": base64.StdEncoding.EncodeToString(raw)})
	var message relayMessage
	if err := json.Unmarshal(encoded, &message); err != nil {
		t.Fatal(err)
	}
	if message.Type != "input" || !bytes.Equal([]byte(message.Data), raw) {
		t.Fatalf("terminal report corrupted: %x", message.Data)
	}
	if err := json.Unmarshal([]byte(`{"type":"input","data":"中文\r"}`), &message); err != nil || message.Data != "中文\r" {
		t.Fatal("browser input compatibility", err, message)
	}
	if err := json.Unmarshal([]byte(`{"type":"resize","cols":120,"rows":40}`), &message); err != nil || message.Cols != 120 || message.Rows != 40 {
		t.Fatal("resize compatibility", err, message)
	}
	for _, value := range []string{"?", base64.StdEncoding.EncodeToString(make([]byte, 65537))} {
		data, _ := json.Marshal(map[string]string{"dataBase64": value})
		if json.Unmarshal(data, &message) == nil {
			t.Fatal("invalid or oversized data accepted")
		}
	}
}
