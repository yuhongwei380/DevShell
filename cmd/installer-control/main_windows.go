package main

import (
	"devshell/internal/installguard"
	"errors"
	"flag"
	"os"
)

func main() {
	directory := flag.String("directory", "", "installation directory")
	payload := flag.String("payload", "", "staged distribution")
	owner := flag.Uint64("owner", 0, "installer HWND")
	silent := flag.Bool("silent", false, "do not prompt or terminate running applications")
	flag.Parse()
	if *directory == "" || *payload == "" {
		os.Exit(1)
	}
	err := installguard.Install(*payload, *directory, uintptr(*owner), *silent)
	if errors.Is(err, installguard.ErrCancelled) {
		os.Exit(1602)
	}
	if err != nil {
		if !*silent {
			installguard.ShowError(uintptr(*owner), err)
		}
		os.Exit(1)
	}
}
