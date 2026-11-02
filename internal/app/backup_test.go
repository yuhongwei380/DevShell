package app

import (
	"bytes"
	"encoding/json"
	"net/http/httptest"
	"os"
	"path/filepath"
	"testing"
	"testing/fstest"
)

func TestSessionBackupExportImportRoundTrip(t *testing.T) {
	source, err := New(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	defer source.Close()
	handler := source.Handler(fstest.MapFS{"index.html": {Data: []byte("fixture")}})
	request := func(method, path string, input any) *httptest.ResponseRecorder {
		t.Helper()
		var body []byte
		if input != nil {
			body, _ = json.Marshal(input)
		}
		req := httptest.NewRequest(method, "http://wails.localhost"+path, bytes.NewReader(body))
		req.Header.Set("X-CloudShell-Token", source.Token())
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, req)
		return response
	}
	key, err := source.store.SaveKey(KeyInput{Name: "backup key", Generate: true, Passphrase: "key-pass"})
	if err != nil {
		t.Fatal(err)
	}
	proxy, err := source.store.SaveProxy(ManagedProxy{Name: "backup proxy", Proxy: ProxyConfig{Type: "socks5", Host: "proxy.example", Port: 1080, User: "pu", Password: "pw"}})
	if err != nil {
		t.Fatal(err)
	}
	if _, err = source.store.Save(Profile{Name: "backup server", Host: "example.com", User: "root", Auth: "key", KeyID: key.ID, Secret: "top-secret", ProxyID: proxy.ID}, false); err != nil {
		t.Fatal(err)
	}
	source.store.mu.Lock()
	source.store.config.HostKeys["known.example"] = "SHA256:knownfingerprint"
	if err = source.store.writeLocked(); err != nil {
		source.store.mu.Unlock()
		t.Fatal(err)
	}
	source.store.mu.Unlock()

	public := request("GET", "/api/backup/export", nil)
	if public.Code != 200 {
		t.Fatalf("export: %d %s", public.Code, public.Body)
	}
	var publicBackup Backup
	if err = json.Unmarshal(public.Body.Bytes(), &publicBackup); err != nil {
		t.Fatal(err)
	}
	if publicBackup.IncludeSecrets || len(publicBackup.Connections) != 1 {
		t.Fatalf("unexpected public backup: %+v", publicBackup)
	}
	if publicBackup.Connections[0].Secret != "" || publicBackup.Keys[0].PrivateKey != "" || publicBackup.Keys[0].Passphrase != "" || !publicBackup.Keys[0].HasPassphrase || publicBackup.Proxies[0].Proxy.Password != "" {
		t.Fatalf("default export leaked secrets: %+v", publicBackup)
	}
	if !publicBackup.Connections[0].HasSecret {
		t.Fatal("public export should flag that a saved password exists")
	}

	secretive := request("GET", "/api/backup/export?secrets=1", nil)
	if secretive.Code != 200 {
		t.Fatalf("secrets export: %d %s", secretive.Code, secretive.Body)
	}
	var backup Backup
	if err = json.Unmarshal(secretive.Body.Bytes(), &backup); err != nil {
		t.Fatal(err)
	}
	if backup.Connections[0].Secret != "top-secret" || backup.Keys[0].Passphrase != "key-pass" || !bytes.Contains([]byte(backup.Keys[0].PrivateKey), []byte("PRIVATE KEY")) {
		t.Fatalf("opt-in export missing secrets: %+v", backup)
	}
	if backup.HostKeys["known.example"] != "SHA256:knownfingerprint" {
		t.Fatalf("host keys not exported: %+v", backup.HostKeys)
	}
	if cd := secretive.Header().Get("Content-Disposition"); !bytes.Contains([]byte(cd), []byte("attachment")) {
		t.Fatalf("export should download as an attachment: %q", cd)
	}

	target, err := New(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	defer target.Close()
	targetRequest := func(path string, input any) (int, []byte) {
		t.Helper()
		body, _ := json.Marshal(input)
		req := httptest.NewRequest("POST", "http://wails.localhost"+path, bytes.NewReader(body))
		req.Header.Set("X-CloudShell-Token", target.Token())
		response := httptest.NewRecorder()
		target.Handler(fstest.MapFS{"index.html": {Data: []byte("fixture")}}).ServeHTTP(response, req)
		return response.Code, response.Body.Bytes()
	}
	code, body := targetRequest("/api/backup/import", backup)
	if code != 200 {
		t.Fatalf("import: %d %s", code, body)
	}
	var summary BackupImportSummary
	if err = json.Unmarshal(body, &summary); err != nil {
		t.Fatal(err)
	}
	if summary.ConnectionsAdded != 1 || summary.KeysAdded != 1 || summary.ProxiesAdded != 1 || summary.HostKeysAdded != 1 {
		t.Fatalf("unexpected import summary: %+v", summary)
	}
	config := target.store.List()
	if len(config.Servers) != 1 || config.Servers[0].Name != "backup server" {
		t.Fatalf("connection not imported: %+v", config.Servers)
	}
	if len(config.Keys) != 1 || config.Keys[0].ID != key.ID {
		t.Fatalf("key identity changed: %+v", config.Keys)
	}
	if _, err = os.Stat(filepath.Join(target.ConfigDirectory(), "keys", key.ID)); err != nil {
		t.Fatalf("private key file not restored: %v", err)
	}
	restored, err := target.store.Get(config.Servers[0].ID)
	if err != nil {
		t.Fatal(err)
	}
	if restored.Secret != "top-secret" {
		t.Fatal("saved password not restored")
	}
	targetKeyData, err := target.store.KeyData(key.ID)
	if err != nil || !bytes.Contains(targetKeyData, []byte("PRIVATE KEY")) {
		t.Fatalf("key material not restored: %v", err)
	}
	target.store.mu.Lock()
	restoredHostKey := target.store.config.HostKeys["known.example"]
	target.store.mu.Unlock()
	if restoredHostKey != "SHA256:knownfingerprint" {
		t.Fatal("host key not restored")
	}

	code, body = targetRequest("/api/backup/import", backup)
	if code != 200 {
		t.Fatalf("repeat import: %d %s", code, body)
	}
	if err = json.Unmarshal(body, &summary); err != nil {
		t.Fatal(err)
	}
	if summary.ConnectionsAdded != 0 || summary.KeysAdded != 0 || summary.ProxiesAdded != 0 || summary.ConnectionsSkipped != 1 || summary.KeysSkipped != 1 {
		t.Fatalf("repeat import must be idempotent: %+v", summary)
	}
	if len(target.store.List().Servers) != 1 {
		t.Fatal("repeat import duplicated connections")
	}

	if code, body = targetRequest("/api/backup/import", map[string]any{"backup": "not-a-backup"}); code == 200 {
		t.Fatalf("foreign file accepted: %s", body)
	}
}

func TestBackupImportAcceptsLegacySchema(t *testing.T) {
	source, err := New(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	defer source.Close()
	if _, err = source.store.Save(Profile{Name: "legacy schema", Host: "example.org", User: "root", Port: 22}, false); err != nil {
		t.Fatal(err)
	}
	raw, err := json.Marshal(source.store.exportBackup(false))
	if err != nil {
		t.Fatal(err)
	}
	var generic map[string]json.RawMessage
	if err = json.Unmarshal(raw, &generic); err != nil {
		t.Fatal(err)
	}
	generic["backup"] = json.RawMessage(`"` + legacyBackupSchema + `"`)
	raw, err = json.Marshal(generic)
	if err != nil {
		t.Fatal(err)
	}
	var imported Backup
	if err = json.Unmarshal(raw, &imported); err != nil {
		t.Fatal(err)
	}
	target, err := New(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	defer target.Close()
	if _, err = target.store.importBackup(imported); err != nil {
		t.Fatalf("legacy backup schema must still import: %v", err)
	}
	if len(target.store.List().Servers) != 1 {
		t.Fatal("connection not imported from legacy-schema backup")
	}
	if source.store.exportBackup(false).Backup != BackupSchema {
		t.Fatal("export must write the current backup schema")
	}
}
