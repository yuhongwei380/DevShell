package app

import (
	"errors"
	"net/http"
	"path/filepath"
	"strings"
	"time"
)

type ConnectionHistory struct {
	ProfileID       string    `json:"profileId"`
	LastConnectedAt time.Time `json:"lastConnectedAt"`
	Count           uint64    `json:"count"`
}

func (s *Store) Delete(id string) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	for i, profile := range s.config.Servers {
		if profile.ID == id {
			if profile.DeletedAt != nil {
				return nil
			}
			now := time.Now().UTC()
			s.config.Servers[i].DeletedAt = &now
			if err := s.writeLocked(); err != nil {
				s.config.Servers[i] = profile
				return err
			}
			return nil
		}
	}
	return errors.New("服务器配置不存在")
}
func (s *Store) Restore(id string) (Profile, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	for i, profile := range s.config.Servers {
		if profile.ID == id {
			if profile.DeletedAt == nil {
				return publicProfile(profile), nil
			}
			// Group/key/proxy deletion guards retain these links while in trash.
			s.config.Servers[i].DeletedAt = nil
			if err := s.writeLocked(); err != nil {
				s.config.Servers[i] = profile
				return Profile{}, err
			}
			return publicProfile(s.config.Servers[i]), nil
		}
	}
	return Profile{}, errors.New("回收站记录不存在")
}
func (s *Store) Purge(id string) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	for _, profile := range s.config.Servers {
		if profile.ID == id {
			if profile.DeletedAt == nil {
				return errors.New("只能永久删除回收站中的服务器")
			}
			return s.purgeLocked(map[string]bool{id: true})
		}
	}
	return errors.New("回收站记录不存在")
}

// PurgeAllTrash backs the 「一键清理」 button: it permanently removes every
// trashed connection with the same cascades as Purge and reports the count.
func (s *Store) PurgeAllTrash() (int, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	ids := map[string]bool{}
	for _, profile := range s.config.Servers {
		if profile.DeletedAt != nil {
			ids[profile.ID] = true
		}
	}
	if len(ids) == 0 {
		return 0, nil
	}
	if err := s.purgeLocked(ids); err != nil {
		return 0, err
	}
	return len(ids), nil
}

// purgeLocked removes every trashed profile named by ids together with its
// connection history, layout slots, path history and directory favorites.
// The caller must hold s.mu.
func (s *Store) purgeLocked(ids map[string]bool) error {
	old := cloneConnectionConfig(s.config)
	kept := make([]Profile, 0, len(s.config.Servers))
	for _, profile := range s.config.Servers {
		if ids[profile.ID] && profile.DeletedAt != nil {
			continue
		}
		kept = append(kept, profile)
	}
	s.config.Servers = kept
	history := make([]ConnectionHistory, 0, len(s.config.ConnectionHistory))
	for _, entry := range s.config.ConnectionHistory {
		if !ids[entry.ProfileID] {
			history = append(history, entry)
		}
	}
	s.config.ConnectionHistory = history
	s.config.Appearance = cloneAppearance(s.config.Appearance)
	for id := range ids {
		delete(s.config.Appearance.Layout, "dengshell.history."+id)
		delete(s.config.Appearance.Layout, "dengshell.nic."+id)
	}
	s.dropPathHistoryLocked(ids)
	s.dropDirectoryFavoritesLocked(ids)
	if err := s.writeLocked(); err != nil {
		s.config = old
		return err
	}
	for id := range ids {
		delete(s.historyState, id)
	}
	return nil
}
func (s *Store) MarkConnected(id string, at time.Time) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	found := false
	for _, profile := range s.config.Servers {
		if profile.ID == id && profile.DeletedAt == nil {
			found = true
			break
		}
	}
	if !found {
		return errors.New("服务器已被删除或移入回收站")
	}
	old := append([]ConnectionHistory{}, s.config.ConnectionHistory...)
	index := -1
	for i, entry := range s.config.ConnectionHistory {
		if entry.ProfileID == id {
			index = i
			break
		}
	}
	if index < 0 {
		s.config.ConnectionHistory = append(s.config.ConnectionHistory, ConnectionHistory{ProfileID: id, LastConnectedAt: at.UTC(), Count: 1})
	} else {
		s.config.ConnectionHistory[index].LastConnectedAt = at.UTC()
		if s.config.ConnectionHistory[index].Count < ^uint64(0) {
			s.config.ConnectionHistory[index].Count++
		}
	}
	if err := s.writeLocked(); err != nil {
		s.config.ConnectionHistory = old
		return err
	}
	return nil
}
func (a *App) trashProfile(id string) error {
	// Serialize deletion with Connect's final history-and-session registration.
	// A connection already dialing cannot appear after its profile was trashed.
	a.mu.Lock()
	if err := a.store.Delete(id); err != nil {
		a.mu.Unlock()
		return err
	}
	closing := []*Session{}
	for sessionID, session := range a.sessions {
		if session.ProfileID == id {
			closing = append(closing, session)
			delete(a.sessions, sessionID)
		}
	}
	a.mu.Unlock()
	for _, session := range closing {
		session.Close()
	}
	a.stopRDPProfile(id)
	a.stopSerialProfile(id)
	return nil
}
func (a *App) registerConnectionManagementHTTP(mux *http.ServeMux) {
	a.registerGroupDeletionHTTP(mux)
	mux.HandleFunc("GET /api/config/storage", func(w http.ResponseWriter, r *http.Request) {
		dir, err := filepath.Abs(a.store.dir)
		if err != nil {
			respond(w, nil, err)
			return
		}
		writeJSON(w, map[string]any{
			"configPath": filepath.Join(dir, EncryptedConfigName), "configDir": dir,
			"keyPath": filepath.Join(dir, ConfigKeyName), "backupPath": filepath.Join(dir, EncryptedConfigName+".bak"),
			"assetsDir": filepath.Join(dir, "assets"), "keysDir": filepath.Join(dir, "keys"),
			"schemaVersion": ConfigSchemaVersion, "envelopeVersion": 1, "format": "AES-256-GCM",
			"encrypted": true, "tamperDetection": true, "automaticUnlock": true,
		})
	})
	mux.HandleFunc("POST /api/group-nodes", func(w http.ResponseWriter, r *http.Request) {
		var group ServerGroup
		if !decode(w, r, &group) {
			return
		}
		saved, err := a.store.SaveGroup(group)
		respond(w, saved, err)
	})
	mux.HandleFunc("POST /api/group-nodes/order", func(w http.ResponseWriter, r *http.Request) {
		var input struct {
			ParentID string   `json:"parentId"`
			IDs      []string `json:"ids"`
		}
		if !decode(w, r, &input) {
			return
		}
		groups, err := a.store.ReorderGroups(input.ParentID, input.IDs)
		respond(w, groups, err)
	})
	mux.HandleFunc("DELETE /api/group-nodes/{id}", func(w http.ResponseWriter, r *http.Request) {
		respond(w, map[string]bool{"ok": true}, a.store.DeleteGroup(r.PathValue("id")))
	})
	mux.HandleFunc("POST /api/group-nodes/{id}/delete-keep-connections", func(w http.ResponseWriter, r *http.Request) {
		var input struct {
			TargetGroupID *string `json:"targetGroupId"`
		}
		if !decode(w, r, &input) {
			return
		}
		target, useParent := "", true
		if input.TargetGroupID != nil {
			target, useParent = strings.TrimSpace(*input.TargetGroupID), false
		}
		moved, err := a.store.DeleteGroupKeepConnections(r.PathValue("id"), target, useParent)
		respond(w, map[string]any{"ok": err == nil, "moved": moved}, err)
	})
	mux.HandleFunc("POST /api/profiles/{id}/group", func(w http.ResponseWriter, r *http.Request) {
		var input struct {
			GroupID string `json:"groupId"`
		}
		if !decode(w, r, &input) {
			return
		}
		profile, err := a.store.MoveProfileGroup(r.PathValue("id"), input.GroupID)
		respond(w, profile, err)
	})
	mux.HandleFunc("POST /api/profiles/{id}/duplicate", func(w http.ResponseWriter, r *http.Request) {
		var input struct {
			Name string `json:"name"`
		}
		if !decode(w, r, &input) {
			return
		}
		profile, err := a.store.Duplicate(r.PathValue("id"), input.Name)
		respond(w, profile, err)
	})
	mux.HandleFunc("POST /api/trash/{id}/restore", func(w http.ResponseWriter, r *http.Request) {
		profile, err := a.store.Restore(r.PathValue("id"))
		respond(w, profile, err)
	})
	mux.HandleFunc("DELETE /api/trash/{id}", func(w http.ResponseWriter, r *http.Request) {
		respond(w, map[string]bool{"ok": true}, a.store.Purge(r.PathValue("id")))
	})
	mux.HandleFunc("POST /api/trash/purge-all", func(w http.ResponseWriter, r *http.Request) {
		removed, err := a.store.PurgeAllTrash()
		respond(w, map[string]any{"ok": err == nil, "removed": removed}, err)
	})
}
