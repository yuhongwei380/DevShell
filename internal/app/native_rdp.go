package app

import "net/http"

// SetNativeUIProcess is called only by the desktop launcher, never an HTTP
// request. RDP child surfaces may attach to that exact GPUI process in addition
// to the historical same-process host. An arbitrary desktop HWND stays invalid.
func (a *App) SetNativeUIProcess(pid int) {
	if pid > 0 {
		a.nativeUIProcess.Store(uint32(pid))
	}
}

func (a *App) validateRDPHost(parent uintptr) error {
	if err := validateRDPParent(parent); err == nil {
		return nil
	}
	return validateRDPParentProcess(parent, a.nativeUIProcess.Load())
}

func (a *App) registerNativeRDPHTTP(mux *http.ServeMux) {
	mux.HandleFunc("POST /api/rdp/connect", func(w http.ResponseWriter, r *http.Request) {
		var input struct {
			ProfileID string `json:"profileId"`
			Secret    string `json:"secret"`
			Parent    uint64 `json:"parent"`
		}
		if !decode(w, r, &input) {
			return
		}
		value, err := a.StartRDPEmbedded(input.ProfileID, input.Secret, uintptr(input.Parent))
		respond(w, value, err)
	})
	mux.HandleFunc("POST /api/rdp/viewport", func(w http.ResponseWriter, r *http.Request) {
		var input struct {
			ID                                 string `json:"id"`
			X, Y, Width, Height, ViewportWidth float64
			Visible                            bool `json:"visible"`
		}
		if !decode(w, r, &input) {
			return
		}
		err := a.RDPViewport(input.ID, input.X, input.Y, input.Width, input.Height, input.ViewportWidth, input.Visible)
		respond(w, map[string]bool{"ok": err == nil}, err)
	})
	mux.HandleFunc("POST /api/rdp/{id}/resolution", func(w http.ResponseWriter, r *http.Request) {
		var input struct {
			Width  int `json:"width"`
			Height int `json:"height"`
		}
		if !decode(w, r, &input) {
			return
		}
		err := a.RDPResolution(r.PathValue("id"), input.Width, input.Height)
		respond(w, map[string]bool{"ok": err == nil}, err)
	})
	mux.HandleFunc("POST /api/rdp/{id}/secure-attention", func(w http.ResponseWriter, r *http.Request) {
		err := a.RDPSendSecureAttention(r.PathValue("id"))
		respond(w, map[string]bool{"ok": err == nil}, err)
	})
}
