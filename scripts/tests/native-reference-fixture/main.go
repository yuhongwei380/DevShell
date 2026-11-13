package main

import (
	"devshell/internal/app"
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"os/signal"
	"path/filepath"
)

func main() {
	root := flag.String("root", "build/ui-qa/reference", "isolated QA directory")
	web := flag.String("web", "build/ui-qa/baseline-v26.11.3-rc.3/web", "reference assets")
	flag.Parse()
	must(os.MkdirAll(*root, 0700))
	store, err := app.OpenStore(filepath.Join(*root, "data"))
	must(err)
	if len(store.List().Servers) == 0 {
		group, err := store.SaveGroup(app.ServerGroup{Name: "开发环境", Emoji: "🛠️"})
		must(err)
		child, err := store.SaveGroup(app.ServerGroup{Name: "应用服务", ParentID: group.ID, Emoji: "📦"})
		must(err)
		for _, profile := range []app.Profile{
			{Name: "Dev Server", Host: "192.0.2.10", User: "root", GroupID: group.ID, Auth: "password", Notes: "开发服务器\n每月 20 日续费"},
			{Name: "Staging API", Host: "192.0.2.11", User: "deploy", GroupID: child.ID, Auth: "agent"},
			{Name: "Windows Desktop", Protocol: "rdp", Host: "192.0.2.12", User: "Administrator", GroupID: group.ID, Port: 3389},
			{Name: "Serial Console", Protocol: "serial", SerialPort: "COM9", BaudRate: 115200, DataBits: 8, Parity: "none", StopBits: 1, FlowControl: "none", GroupID: group.ID},
		} {
			_, err = store.Save(profile, false)
			must(err)
		}
	}
	application, err := app.New(filepath.Join(*root, "data"))
	must(err)
	defer application.Close()
	must(application.Start("127.0.0.1:0", os.DirFS(*web)))
	// Generate a real, uncommitted enrollment QR in this isolated QA store.
	// Do not enable the lock or save the ephemeral secret to its policy.
	grant, err := application.AuthorizeLockSettings(app.LockProof{})
	must(err)
	enrollment, err := application.EnrollLockTOTP(grant)
	must(err)
	fixture := map[string]any{"url": application.BrowserURL(), "base": application.URL(), "token": application.Token(), "enrollment": enrollment}
	bytes, _ := json.Marshal(fixture)
	must(os.WriteFile(filepath.Join(*root, "fixture.json"), bytes, 0600))
	fmt.Println("Reference fixture ready")
	signals := make(chan os.Signal, 1)
	signal.Notify(signals, os.Interrupt)
	<-signals
}
func must(err error) {
	if err != nil {
		panic(err)
	}
}
