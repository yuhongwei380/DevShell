package main

import (
	"devshell/internal/app"
	"devshell/internal/installguard"
	"devshell/internal/nativeui"
	"devshell/internal/updater"
	"embed"
	"errors"
	"flag"
	"io/fs"
	"log"
	"os"
	"time"
)

//go:embed web
var assets embed.FS

func main() {
	if updater.HandleFlag() {
		return
	}
	if err := installguard.CheckLaunch(); err != nil {
		// Do not leave this executable mapped behind a blocking error dialog
		// while the installer is waiting to replace it.
		if !errors.Is(err, installguard.ErrInstalling) {
			showStartupError(err.Error())
		}
		log.Print(err)
		return
	}
	address := flag.String("addr", "127.0.0.1:0", "local API listen address")
	dev := flag.Bool("dev", false, "read frontend assets from the web directory")
	config := flag.String("config", "", "configuration directory (default: data beside executable; macOS app: Application Support/DevShell)")
	flag.Parse()
	var content fs.FS
	var contentErr error
	if *dev {
		content = os.DirFS("web")
	} else {
		content, contentErr = fs.Sub(assets, "web")
		if contentErr != nil {
			log.Fatal(contentErr)
		}
	}
	if *config == "" {
		var err error
		*config, err = nativeui.PortableConfigDir()
		if err != nil {
			log.Fatal(err)
		}
	}
	if err := app.MigratePortableData(*config); err != nil {
		showStartupError(err.Error())
		log.Fatal(err)
	}
	application, err := app.New(*config)
	if err != nil {
		showStartupError(err.Error())
		log.Fatal(err)
	}
	defer application.Close()
	application.SetInstallUpdateHandler(func(id string) error {
		job, err := application.PreparedUpdate(id)
		if err != nil {
			return err
		}
		err = updater.LaunchUpdateHelper(job, *config, nil)
		var pending *updater.ExitPendingError
		if err != nil && !errors.As(err, &pending) {
			return err
		}
		go func() {
			time.Sleep(250 * time.Millisecond)
			os.Exit(0)
		}()
		return nil
	})
	if err = application.Start(*address, content); err != nil {
		log.Fatal(err)
	}
	if err := nativeui.Run(application.URL(), application.Token()); err != nil {
		showStartupError(err.Error())
		log.Print(err)
	}
}
