package app

import (
	"bytes"
	"encoding/json"
	"net/http/httptest"
	"os"
	"path/filepath"
	"reflect"
	"testing"
	"testing/fstest"
)

func TestMoveProfileGroupPreservesConfigAndRollsBack(t *testing.T) {
	s, err := OpenStore(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	source, _ := s.SaveGroup(ServerGroup{Name: "来源"})
	parent, _ := s.SaveGroup(ServerGroup{Name: "目标"})
	target, _ := s.SaveGroup(ServerGroup{Name: "子组", ParentID: parent.ID})
	p, err := s.Save(Profile{Name: "保留配置", Host: "192.0.2.1", User: "root", GroupID: source.ID, Secret: "ssh-secret", Notes: "备注", Proxy: ProxyConfig{Type: "socks5", Host: "localhost", Port: 1080, Password: "proxy-secret"}}, false)
	if err != nil {
		t.Fatal(err)
	}
	original, _ := s.Get(p.ID)
	before := cloneConnectionConfig(s.config)
	for _, input := range [][2]string{{p.ID, "missing"}, {"missing", target.ID}} {
		if _, err := s.MoveProfileGroup(input[0], input[1]); err == nil {
			t.Fatal("invalid move succeeded")
		}
		if !reflect.DeepEqual(before, cloneConnectionConfig(s.config)) {
			t.Fatal("rejected move changed config")
		}
	}
	blocked := filepath.Join(t.TempDir(), "file")
	if err := os.WriteFile(blocked, []byte("blocked"), 0600); err != nil {
		t.Fatal(err)
	}
	oldDir := s.dir
	s.dir = blocked
	_, err = s.MoveProfileGroup(p.ID, target.ID)
	s.dir = oldDir
	if err == nil || !reflect.DeepEqual(before, cloneConnectionConfig(s.config)) {
		t.Fatal("failed write did not roll back")
	}
	if free, err := s.MoveProfileGroup(p.ID, ""); err != nil || free.GroupID != "" || free.Group != "" {
		t.Fatalf("move to ungrouped failed: %+v %v", free, err)
	}
	if free, _ := s.Get(p.ID); free.Secret != "ssh-secret" || free.Proxy.Password != "proxy-secret" || free.Notes != "备注" {
		t.Fatal("move to ungrouped changed connection fields")
	}
	public, err := s.MoveProfileGroup(p.ID, target.ID)
	if err != nil {
		t.Fatal(err)
	}
	if public.GroupID != target.ID || public.Group != "目标 / 子组" || public.Secret != "" || !public.HasSecret || public.Proxy.Password != "" || !public.Proxy.HasPassword {
		t.Fatal("move response lost grouping or exposed credentials")
	}
	reopened, err := OpenStore(s.dir)
	if err != nil {
		t.Fatal(err)
	}
	stored, err := reopened.Get(p.ID)
	original.GroupID, original.Group = target.ID, "目标 / 子组"
	if err != nil || !reflect.DeepEqual(original, stored) {
		t.Fatal("move changed connection fields or did not persist")
	}
	if _, err := s.MoveProfileGroup(p.ID, target.ID); err != nil {
		t.Fatal("same group move failed", err)
	}
	if err := s.Delete(p.ID); err != nil {
		t.Fatal(err)
	}
	before = cloneConnectionConfig(s.config)
	if _, err := s.MoveProfileGroup(p.ID, source.ID); err == nil || !reflect.DeepEqual(before, cloneConnectionConfig(s.config)) {
		t.Fatal("deleted connection was moved")
	}
}

func TestMoveProfileGroupHTTP(t *testing.T) {
	a, err := New(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	defer a.Close()
	source, _ := a.store.SaveGroup(ServerGroup{Name: "来源"})
	target, _ := a.store.SaveGroup(ServerGroup{Name: "目标"})
	p, err := a.store.Save(Profile{Name: "连接", Host: "192.0.2.1", User: "root", GroupID: source.ID, Secret: "private-value"}, false)
	if err != nil {
		t.Fatal(err)
	}
	handler := a.Handler(fstest.MapFS{"index.html": {Data: []byte("fixture")}})
	for _, groupID := range []string{"missing", target.ID, ""} {
		body, _ := json.Marshal(map[string]string{"groupId": groupID})
		req := httptest.NewRequest("POST", "http://wails.localhost/api/profiles/"+p.ID+"/group", bytes.NewReader(body))
		req.Header.Set("X-CloudShell-Token", a.Token())
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, req)
		if groupID == "missing" {
			if response.Code == 200 {
				t.Fatal("invalid group accepted")
			}
			continue
		}
		var moved Profile
		if err := json.Unmarshal(response.Body.Bytes(), &moved); err != nil || response.Code != 200 || !moved.HasSecret || bytes.Contains(response.Body.Bytes(), []byte("private-value")) {
			t.Fatal("incorrect move HTTP response", response.Code)
		}
		if moved.GroupID != groupID {
			t.Fatal("incorrect move target", moved.GroupID)
		}
	}
}

func TestServerMoveBrowser(t *testing.T) {
	runIsolatedNavigationBrowser(t, "test-server-move-browser.mjs")
}
