package main

import (
	"devshell/internal/app"
	"devshell/internal/desktop"
	"devshell/internal/installguard"
	"devshell/internal/nativeui"
	"devshell/internal/updater"
	"embed"
	"errors"
	"flag"
	"io/fs"
	"log"
	"os"
	"os/signal"
	"syscall"
	"time"
)

//go:embed web
var assets embed.FS

//go:embed build/devshell.png
var desktopIcon []byte

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
	detached := flag.Bool("detached-window", false, "internal independent window")
	browser := flag.Bool("browser", false, "run local browser mode instead of a desktop window")
	native := flag.Bool("native", false, "run the native GPUI frontend")
	address := flag.String("addr", "127.0.0.1:0", "local API listen address")
	dev := flag.Bool("dev", false, "read frontend assets from the web directory")
	config := flag.String("config", "", "configuration directory (default: data beside executable; macOS app: Application Support/DevShell)")
	flag.Parse()
	nativeExplicit := false
	flag.Visit(func(value *flag.Flag) {
		if value.Name == "native" {
			nativeExplicit = true
		}
	})
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
	if *detached {
		if err := desktop.RunDetached(content, desktopIcon); err != nil {
			showStartupError(err.Error())
			log.Print(err)
		}
		return
	}

	if *config == "" {
		var err error
		*config, err = desktop.PortableConfigDir()
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
	// Release builds place DevShellUI beside the Go process. Prefer it
	// automatically so a packaged desktop launch never starts WebView2; the
	// legacy desktop frontend remains available only when no native binary is
	// present or when the caller explicitly passes --native=false.
	if !*browser && !nativeExplicit && nativeui.Available() {
		*native = true
	}
	if *native {
		if err := nativeui.Run(application.URL(), application.Token()); err != nil {
			showStartupError(err.Error())
			log.Print(err)
		}
		return
	}
	if !*browser && desktop.Available() {
		log.Printf("DevShell: %s", application.URL())
		if err = desktop.Run(application, content, *config, desktopIcon); err != nil {
			log.Print(err)
		}
		return
	}
	log.Printf("DevShell: %s", application.BrowserURL())
	log.Print("本地浏览器模式：请打开上面的地址")
	signals := make(chan os.Signal, 1)
	signal.Notify(signals, syscall.SIGINT, syscall.SIGTERM)
	<-signals
}
