package app

import (
	"bytes"
	"encoding/json"
	"net/http/httptest"
	"os"
	"path/filepath"
	"runtime"
	"testing"
	"testing/fstest"
)

func TestConnectionManagementHTTPContract(t *testing.T) {
	a, err := New(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	defer a.Close()
	handler := a.Handler(fstest.MapFS{"index.html": {Data: []byte("fixture")}})
	request := func(method, path string, input any) *httptest.ResponseRecorder {
		t.Helper()
		body, _ := json.Marshal(input)
		req := httptest.NewRequest(method, "http://wails.localhost"+path, bytes.NewReader(body))
		req.Header.Set("X-CloudShell-Token", a.Token())
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, req)
		if response.Code != 200 {
			t.Fatalf("%s %s: %d %s", method, path, response.Code, response.Body)
		}
		return response
	}
	response := request("GET", "/api/config/storage", nil)
	var storage struct {
		ConfigPath    string `json:"configPath"`
		ConfigDir     string `json:"configDir"`
		SchemaVersion int    `json:"schemaVersion"`
	}
	if err := json.Unmarshal(response.Body.Bytes(), &storage); err != nil {
		t.Fatal(err)
	}
	if storage.ConfigDir != a.store.dir || storage.ConfigPath != filepath.Join(a.store.dir, EncryptedConfigName) || storage.SchemaVersion != 2 {
		t.Fatalf("incorrect storage metadata: %+v", storage)
	}
	response = request("POST", "/api/group-nodes", ServerGroup{Name: "HTTP parent", Emoji: "🛰️"})
	var group ServerGroup
	if err := json.Unmarshal(response.Body.Bytes(), &group); err != nil {
		t.Fatal(err)
	}
	response = request("POST", "/api/profiles", Profile{Name: "HTTP server", Host: "localhost", User: "root", GroupID: group.ID, Secret: "kept-private"})
	var profile Profile
	if err := json.Unmarshal(response.Body.Bytes(), &profile); err != nil {
		t.Fatal(err)
	}
	if profile.GroupID != group.ID || profile.Secret != "" || !profile.HasSecret {
		t.Fatal("save contract lost grouping/secret metadata")
	}
	request("DELETE", "/api/profiles/"+profile.ID, nil)
	response = request("GET", "/api/config", nil)
	var config Config
	if err := json.Unmarshal(response.Body.Bytes(), &config); err != nil {
		t.Fatal(err)
	}
	if len(config.Servers) != 0 || len(config.Trash) != 1 || config.Trash[0].DeletedAt == nil || bytes.Contains(response.Body.Bytes(), []byte("kept-private")) {
		t.Fatal("soft-delete API did not redact or partition profiles")
	}
	response = request("POST", "/api/trash/"+profile.ID+"/restore", nil)
	if bytes.Contains(response.Body.Bytes(), []byte("kept-private")) {
		t.Fatal("restore HTTP response disclosed password")
	}
	request("DELETE", "/api/profiles/"+profile.ID, nil)
	request("DELETE", "/api/trash/"+profile.ID, nil)
	request("DELETE", "/api/group-nodes/"+group.ID, nil)
}

// The UI promotes a credential typed at connect time by re-posting the public
// profile view together with the secret. That round trip has to persist the
// secret while keeping the inline proxy password the public view never reveals.
func TestCredentialWriteBackRoundTripThroughPublicProfile(t *testing.T) {
	a, err := New(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	defer a.Close()
	handler := a.Handler(fstest.MapFS{"index.html": {Data: []byte("fixture")}})
	request := func(method, path string, input any) *httptest.ResponseRecorder {
		t.Helper()
		body, _ := json.Marshal(input)
		req := httptest.NewRequest(method, "http://wails.localhost"+path, bytes.NewReader(body))
		req.Header.Set("X-CloudShell-Token", a.Token())
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, req)
		if response.Code != 200 {
			t.Fatalf("%s %s: %d %s", method, path, response.Code, response.Body)
		}
		return response
	}
	created := request("POST", "/api/profiles", Profile{Name: "写回凭据", Host: "203.0.113.9", User: "root", Auth: "password", Proxy: ProxyConfig{Type: "socks5", Host: "127.0.0.1", Port: 1080, User: "proxy-user", Password: "inline-proxy-password"}})
	var public map[string]any
	if err := json.Unmarshal(created.Body.Bytes(), &public); err != nil {
		t.Fatal(err)
	}
	if _, found := public["secret"]; found {
		t.Fatal("created profile disclosed a secret field")
	}
	if proxy, _ := public["proxy"].(map[string]any); proxy["hasPassword"] != true {
		t.Fatal("created profile lost the proxy password indicator")
	}
	// Exactly what the frontend spreads back: the public object plus the typed
	// credential and an explicit "do not clear".
	public["secret"] = "typed-at-connect"
	public["clearSecret"] = false
	response := request("POST", "/api/profiles", public)
	var saved Profile
	if err := json.Unmarshal(response.Body.Bytes(), &saved); err != nil {
		t.Fatal(err)
	}
	if !saved.HasSecret || saved.Secret != "" {
		t.Fatalf("write-back response lost the credential indicator: %+v", saved)
	}
	if bytes.Contains(response.Body.Bytes(), []byte("typed-at-connect")) || bytes.Contains(response.Body.Bytes(), []byte("inline-proxy-password")) {
		t.Fatal("save response disclosed credentials")
	}
	stored, err := a.store.Get(saved.ID)
	if err != nil || stored.Secret != "typed-at-connect" || stored.Proxy.Password != "inline-proxy-password" {
		t.Fatalf("write-back lost the credential or the stored proxy password: %+v err=%v", stored, err)
	}
	// The retry path replaces a stored credential the server rejected, again by
	// re-posting nothing but the public view plus the credential that worked.
	var retry map[string]any
	if err := json.Unmarshal(response.Body.Bytes(), &retry); err != nil {
		t.Fatal(err)
	}
	retry["secret"] = "replacement-after-rejection"
	retry["clearSecret"] = false
	response = request("POST", "/api/profiles", retry)
	if bytes.Contains(response.Body.Bytes(), []byte("replacement-after-rejection")) || bytes.Contains(response.Body.Bytes(), []byte("inline-proxy-password")) {
		t.Fatal("replacement response disclosed credentials")
	}
	if stored, err = a.store.Get(saved.ID); err != nil || stored.Secret != "replacement-after-rejection" || stored.Proxy.Password != "inline-proxy-password" {
		t.Fatalf("replacing a rejected credential did not persist: %+v err=%v", stored, err)
	}
	reopened, err := OpenStore(a.store.dir)
	if err != nil {
		t.Fatal(err)
	}
	if p, err := reopened.Get(saved.ID); err != nil || p.Secret != "replacement-after-rejection" || p.Proxy.Password != "inline-proxy-password" {
		t.Fatalf("write-back did not survive restart: %+v err=%v", p, err)
	}
}
func TestConfigMigrationRefusesToProceedWithoutBackup(t *testing.T) {
	if runtime.GOOS == "windows" || os.Geteuid() == 0 {
		t.Skip("requires nonroot POSIX directory permissions")
	}
	dir := t.TempDir()
	path := filepath.Join(dir, "config.json")
	original := []byte(`{"servers":[],"groups":["保留"]}`)
	if err := os.WriteFile(path, original, 0600); err != nil {
		t.Fatal(err)
	}
	if err := os.Chmod(dir, 0500); err != nil {
		t.Fatal(err)
	}
	defer os.Chmod(dir, 0700)
	if _, err := OpenStore(dir); err == nil {
		t.Fatal("migration continued without writing required backup")
	}
	current, err := os.ReadFile(path)
	if err != nil || !bytes.Equal(current, original) {
		t.Fatal("migration modified original after backup failure", err)
	}
}

func TestProfileDuplicateCarriesStoredCredentials(t *testing.T) {
	a, err := New(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	defer a.Close()
	handler := a.Handler(fstest.MapFS{"index.html": {Data: []byte("fixture")}})
	request := func(method, path string, input any) *httptest.ResponseRecorder {
		t.Helper()
		body, _ := json.Marshal(input)
		req := httptest.NewRequest(method, "http://wails.localhost"+path, bytes.NewReader(body))
		req.Header.Set("X-CloudShell-Token", a.Token())
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, req)
		if response.Code != 200 {
			t.Fatalf("%s %s: %d %s", method, path, response.Code, response.Body)
		}
		return response
	}
	response := request("POST", "/api/group-nodes", ServerGroup{Name: "Duplicate parent"})
	var group ServerGroup
	if err := json.Unmarshal(response.Body.Bytes(), &group); err != nil {
		t.Fatal(err)
	}
	response = request("POST", "/api/profiles", Profile{
		Name:    "Duplicate source",
		Host:    "localhost",
		User:    "root",
		GroupID: group.ID,
		Secret:  "dup-login-secret",
		Proxy:   ProxyConfig{Type: "http", Host: "proxy.invalid", Port: 8080, User: "proxy-user", Password: "dup-proxy-secret"},
	})
	var source Profile
	if err := json.Unmarshal(response.Body.Bytes(), &source); err != nil {
		t.Fatal(err)
	}
	if !source.HasSecret || !source.Proxy.HasPassword {
		t.Fatalf("source credential metadata lost: %+v", source)
	}
	response = request("POST", "/api/profiles/"+source.ID+"/duplicate", struct{}{})
	if bytes.Contains(response.Body.Bytes(), []byte("dup-login-secret")) || bytes.Contains(response.Body.Bytes(), []byte("dup-proxy-secret")) {
		t.Fatal("duplicate response disclosed credentials")
	}
	var clone Profile
	if err := json.Unmarshal(response.Body.Bytes(), &clone); err != nil {
		t.Fatal(err)
	}
	if clone.ID == "" || clone.ID == source.ID || clone.Name != source.Name || clone.GroupID != group.ID {
		t.Fatalf("unexpected clone identity: %+v", clone)
	}
	if !clone.HasSecret || !clone.Proxy.HasPassword {
		t.Fatalf("clone lost credential metadata: %+v", clone)
	}
	stored, err := a.store.Get(clone.ID)
	if err != nil || stored.Secret != "dup-login-secret" || stored.Proxy.Password != "dup-proxy-secret" {
		t.Fatalf("clone did not carry the stored credentials: %+v err=%v", stored, err)
	}
	if _, err := a.store.Get(source.ID); err != nil {
		t.Fatalf("source must survive the duplicate: %v", err)
	}
	request("DELETE", "/api/profiles/"+source.ID, nil)
	body, _ := json.Marshal(struct{}{})
	req := httptest.NewRequest("POST", "http://wails.localhost/api/profiles/"+source.ID+"/duplicate", bytes.NewReader(body))
	req.Header.Set("X-CloudShell-Token", a.Token())
	trashed := httptest.NewRecorder()
	handler.ServeHTTP(trashed, req)
	if trashed.Code == 200 {
		t.Fatalf("duplicating a trashed profile succeeded: %s", trashed.Body)
	}
}

// 「复制链接」提交 原名-clone；链接栏与服务器管理共用同一份配置，
// 名称必须保持唯一（重名时 store 追加 2/3… 后缀）。空 name 保留旧的
// 原样复制行为；克隆渠道端点对不存在的会话必须干净失败。
func TestProfileDuplicateCloneNamesAndChannelEndpoint(t *testing.T) {
	a, err := New(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	defer a.Close()
	handler := a.Handler(fstest.MapFS{"index.html": {Data: []byte("fixture")}})
	request := func(method, path string, input any) *httptest.ResponseRecorder {
		t.Helper()
		body, _ := json.Marshal(input)
		req := httptest.NewRequest(method, "http://wails.localhost"+path, bytes.NewReader(body))
		req.Header.Set("X-CloudShell-Token", a.Token())
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, req)
		if response.Code != 200 {
			t.Fatalf("%s %s: %d %s", method, path, response.Code, response.Body)
		}
		return response
	}
	response := request("POST", "/api/profiles", Profile{Name: "Clone source", Host: "localhost", User: "root", Secret: "clone-secret"})
	var source Profile
	if err := json.Unmarshal(response.Body.Bytes(), &source); err != nil {
		t.Fatal(err)
	}
	for _, want := range []string{"Clone source-clone", "Clone source-clone 2", "Clone source-clone 3"} {
		response = request("POST", "/api/profiles/"+source.ID+"/duplicate", struct {
			Name string `json:"name"`
		}{Name: "Clone source-clone"})
		if bytes.Contains(response.Body.Bytes(), []byte("clone-secret")) {
			t.Fatal("named duplicate response disclosed credentials")
		}
		var clone Profile
		if err := json.Unmarshal(response.Body.Bytes(), &clone); err != nil {
			t.Fatal(err)
		}
		if clone.Name != want {
			t.Fatalf("named duplicate = %q, want %q", clone.Name, want)
		}
	}
	response = request("POST", "/api/profiles/"+source.ID+"/duplicate", struct{}{})
	var bare Profile
	if err := json.Unmarshal(response.Body.Bytes(), &bare); err != nil {
		t.Fatal(err)
	}
	if bare.Name != "Clone source" {
		t.Fatalf("unnamed duplicate renamed: %q", bare.Name)
	}
	body, _ := json.Marshal(struct{}{})
	req := httptest.NewRequest("POST", "http://wails.localhost/api/sessions/missing-session/channel", bytes.NewReader(body))
	req.Header.Set("X-CloudShell-Token", a.Token())
	missing := httptest.NewRecorder()
	handler.ServeHTTP(missing, req)
	if missing.Code == 200 {
		t.Fatalf("clone channel for a nonexistent session succeeded: %s", missing.Body)
	}
}
