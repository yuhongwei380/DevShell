package app

import (
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"time"
)

// Session backups move connections, groups, managed keys, proxies and known
// host keys between installations. Secrets (saved passwords, proxy passwords,
// private key files and key passphrases) are only included when explicitly
// requested, because the backup file itself is plaintext JSON.
const BackupSchema = "dengshell.backup.v1"

const (
	backupMaxConnections = 2000
	backupMaxKeys        = 200
	backupMaxProxies     = 200
	backupMaxHostKeys    = 2000
	backupMaxKeyBytes    = 2 << 20
	backupMaxBodyBytes   = 32 << 20
)

type BackupKey struct {
	ManagedKey
	PrivateKey string `json:"privateKey,omitempty"`
}

type Backup struct {
	Backup         string              `json:"backup"`
	ExportedAt     time.Time           `json:"exportedAt"`
	AppVersion     string              `json:"appVersion"`
	IncludeSecrets bool                `json:"includeSecrets"`
	Connections    []Profile           `json:"connections"`
	Groups         []string            `json:"groups,omitempty"`
	GroupNodes     []ServerGroup       `json:"groupNodes,omitempty"`
	Keys           []BackupKey         `json:"keys,omitempty"`
	Proxies        []ManagedProxy      `json:"proxies,omitempty"`
	HostKeys       map[string]string   `json:"hostKeys,omitempty"`
}

type BackupImportSummary struct {
	ConnectionsAdded   int `json:"connectionsAdded"`
	ConnectionsSkipped int `json:"connectionsSkipped"`
	GroupsAdded        int `json:"groupsAdded"`
	KeysAdded          int `json:"keysAdded"`
	KeysSkipped        int `json:"keysSkipped"`
	ProxiesAdded       int `json:"proxiesAdded"`
	ProxiesSkipped     int `json:"proxiesSkipped"`
	HostKeysAdded      int `json:"hostKeysAdded"`
}

func (s *Store) exportBackup(includeSecrets bool) Backup {
	s.mu.Lock()
	defer s.mu.Unlock()
	backup := Backup{Backup: BackupSchema, ExportedAt: time.Now().UTC(), AppVersion: ApplicationVersion, IncludeSecrets: includeSecrets}
	for _, profile := range s.config.Servers {
		if profile.DeletedAt != nil || profile.Temporary {
			continue
		}
		profile.Verbose = 0
		profile.Proxy.ClearPassword = false
		if !includeSecrets {
			profile.HasSecret = profile.Secret != ""
			profile.Secret = ""
			profile.Proxy.HasPassword = profile.Proxy.Password != ""
			profile.Proxy.Password = ""
		} else {
			profile.HasSecret = false
			profile.Proxy.HasPassword = false
		}
		backup.Connections = append(backup.Connections, profile)
	}
	backup.Groups = append([]string{}, s.config.Groups...)
	backup.GroupNodes = append([]ServerGroup{}, s.config.GroupNodes...)
	for _, key := range s.config.Keys {
		entry := BackupKey{ManagedKey: key}
		if !includeSecrets {
			entry.Passphrase = ""
			entry.HasPassphrase = key.Passphrase != ""
		}
		if includeSecrets {
			if data, err := readPrivateKey(filepath.Join(s.dir, "keys", key.ID)); err == nil && len(data) <= backupMaxKeyBytes {
				entry.PrivateKey = string(data)
			}
		}
		backup.Keys = append(backup.Keys, entry)
	}
	for _, proxy := range s.config.Proxies {
		proxy.Proxy.ClearPassword = false
		if !includeSecrets {
			proxy.Proxy.HasPassword = proxy.Proxy.Password != ""
			proxy.Proxy.Password = ""
		} else {
			proxy.Proxy.HasPassword = false
		}
		backup.Proxies = append(backup.Proxies, proxy)
	}
	if len(s.config.HostKeys) > 0 {
		backup.HostKeys = make(map[string]string, len(s.config.HostKeys))
		for host, fingerprint := range s.config.HostKeys {
			backup.HostKeys[host] = fingerprint
		}
	}
	return backup
}

func validateBackup(backup Backup) error {
	if backup.Backup != BackupSchema {
		return errors.New("文件不是 DevShell 会话备份")
	}
	if len(backup.Connections) > backupMaxConnections || len(backup.Keys) > backupMaxKeys || len(backup.Proxies) > backupMaxProxies || len(backup.HostKeys) > backupMaxHostKeys {
		return errors.New("备份内容超出允许范围")
	}
	for _, profile := range backup.Connections {
		profile.Name = strings.TrimSpace(profile.Name)
		profile.Host = strings.TrimSpace(profile.Host)
		if profile.ID == "" || len(profile.ID) > 64 || profile.DeletedAt != nil || profile.Temporary {
			return errors.New("备份包含异常连接条目")
		}
		switch profile.Protocol {
		case "", "ssh", "rdp", "serial":
		default:
			return fmt.Errorf("备份包含未知协议：%s", profile.Protocol)
		}
		if profile.Protocol != "serial" && (profile.Name == "" || profile.Host == "" || profile.User == "") {
			return errors.New("备份包含缺少名称、地址或用户名的连接")
		}
		if profile.Protocol == "serial" && profile.Host == "" {
			return errors.New("备份包含缺少串口的连接")
		}
		if profile.Port < 0 || profile.Port > 65535 {
			return errors.New("备份包含无效端口")
		}
	}
	for _, key := range backup.Keys {
		if key.ID == "" || len(key.ID) > 64 {
			return errors.New("备份包含异常密钥条目")
		}
		if len(key.PrivateKey) > backupMaxKeyBytes {
			return errors.New("备份中的私钥过大")
		}
		if key.PrivateKey != "" && !strings.Contains(key.PrivateKey, "PRIVATE KEY") {
			return errors.New("备份中的私钥格式不正确")
		}
	}
	for _, proxy := range backup.Proxies {
		if proxy.ID == "" || len(proxy.ID) > 64 {
			return errors.New("备份包含异常代理条目")
		}
	}
	return nil
}

func (s *Store) importBackup(backup Backup) (BackupImportSummary, error) {
	if err := validateBackup(backup); err != nil {
		return BackupImportSummary{}, err
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	var summary BackupImportSummary
	old := cloneConnectionConfig(s.config)
	committed := false
	defer func() {
		if !committed {
			s.config = old
		}
	}()
	knownKeyIDs := map[string]bool{}
	keyFingerprints := map[string]string{}
	for _, key := range s.config.Keys {
		knownKeyIDs[key.ID] = true
		if key.Fingerprint != "" {
			keyFingerprints[key.Fingerprint] = key.ID
		}
	}
	keyIDMap := map[string]string{}
	keysDir := filepath.Join(s.dir, "keys")
	for _, entry := range backup.Keys {
		if existing, seen := keyFingerprints[entry.Fingerprint]; seen && entry.Fingerprint != "" {
			keyIDMap[entry.ID] = existing
			summary.KeysSkipped++
			continue
		}
		id := entry.ID
		if knownKeyIDs[id] {
			id = randomID()
		}
		keyIDMap[entry.ID] = id
		migrated := entry.ManagedKey
		migrated.ID = id
		migrated.HasPassphrase = false
		if migrated.CreatedAt.IsZero() {
			migrated.CreatedAt = time.Now().UTC()
		}
		if entry.PrivateKey != "" {
			if err := os.MkdirAll(keysDir, 0700); err != nil {
				return BackupImportSummary{}, err
			}
			if err := os.WriteFile(filepath.Join(keysDir, id), []byte(entry.PrivateKey), 0600); err != nil {
				return BackupImportSummary{}, err
			}
		}
		s.config.Keys = append(s.config.Keys, migrated)
		knownKeyIDs[id] = true
		if migrated.Fingerprint != "" {
			keyFingerprints[migrated.Fingerprint] = id
		}
		summary.KeysAdded++
	}
	knownProxyIDs := map[string]bool{}
	proxySignature := func(value ProxyConfig) string {
		return strings.Join([]string{value.Type, value.Host, fmt.Sprint(value.Port), value.User}, "|")
	}
	proxySignatures := map[string]string{}
	for _, proxy := range s.config.Proxies {
		knownProxyIDs[proxy.ID] = true
		proxySignatures[proxySignature(proxy.Proxy)] = proxy.ID
	}
	proxyIDMap := map[string]string{}
	for _, proxy := range backup.Proxies {
		if existing, seen := proxySignatures[proxySignature(proxy.Proxy)]; seen {
			proxyIDMap[proxy.ID] = existing
			summary.ProxiesSkipped++
			continue
		}
		id := proxy.ID
		if knownProxyIDs[id] {
			id = randomID()
		}
		proxyIDMap[proxy.ID] = id
		proxy.ID = id
		proxy.Proxy.HasPassword = false
		proxy.Proxy.ClearPassword = false
		s.config.Proxies = append(s.config.Proxies, proxy)
		knownProxyIDs[id] = true
		proxySignatures[proxySignature(proxy.Proxy)] = id
		summary.ProxiesAdded++
	}
	existingProfileIDs := map[string]bool{}
	profileSignatures := map[string]bool{}
	for _, profile := range s.config.Servers {
		existingProfileIDs[profile.ID] = true
		profileSignatures[strings.Join([]string{profile.Name, profile.Host, fmt.Sprint(profile.Port), profile.User}, "|")] = true
	}
	for _, profile := range backup.Connections {
		signature := strings.Join([]string{strings.TrimSpace(profile.Name), strings.TrimSpace(profile.Host), fmt.Sprint(profile.Port), strings.TrimSpace(profile.User)}, "|")
		if existingProfileIDs[profile.ID] || profileSignatures[signature] {
			summary.ConnectionsSkipped++
			continue
		}
		if profile.KeyID != "" {
			if mapped, seen := keyIDMap[profile.KeyID]; seen {
				profile.KeyID = mapped
			} else if !knownKeyIDs[profile.KeyID] {
				// The referenced managed key is not part of this backup; drop
				// the dangling reference instead of importing a broken profile.
				profile.KeyID = ""
				if profile.KeyPath == "" && profile.Auth == "key" {
					profile.Auth = "password"
				}
			}
		}
		if profile.ProxyID != "" {
			if mapped, seen := proxyIDMap[profile.ProxyID]; seen {
				profile.ProxyID = mapped
			} else if !knownProxyIDs[profile.ProxyID] {
				profile.ProxyID = ""
			}
		}
		profile.Verbose = 0
		profile.Temporary = false
		profile.DeletedAt = nil
		profile.HasSecret = false
		profile.Proxy.HasPassword = false
		profile.Proxy.ClearPassword = false
		s.config.Servers = append(s.config.Servers, profile)
		existingProfileIDs[profile.ID] = true
		profileSignatures[signature] = true
		summary.ConnectionsAdded++
	}
	knownGroups := map[string]bool{}
	for _, group := range s.config.Groups {
		knownGroups[group] = true
	}
	for _, group := range backup.Groups {
		group = strings.TrimSpace(group)
		if group == "" || knownGroups[group] {
			continue
		}
		s.config.Groups = append(s.config.Groups, group)
		knownGroups[group] = true
		summary.GroupsAdded++
	}
	existingNodeIDs := map[string]bool{}
	for _, node := range s.config.GroupNodes {
		existingNodeIDs[node.ID] = true
	}
	for _, node := range backup.GroupNodes {
		if node.ID == "" || existingNodeIDs[node.ID] {
			continue
		}
		s.config.GroupNodes = append(s.config.GroupNodes, node)
		existingNodeIDs[node.ID] = true
	}
	if s.config.HostKeys == nil {
		s.config.HostKeys = map[string]string{}
	}
	for host, fingerprint := range backup.HostKeys {
		host = strings.TrimSpace(host)
		if host == "" || fingerprint == "" {
			continue
		}
		if _, exists := s.config.HostKeys[host]; exists {
			continue
		}
		s.config.HostKeys[host] = fingerprint
		summary.HostKeysAdded++
	}
	s.refreshLegacyGroupsLocked()
	if err := s.writeLocked(); err != nil {
		return BackupImportSummary{}, err
	}
	committed = true
	return summary, nil
}

func (a *App) registerBackupRoutes(mux *http.ServeMux) {
	mux.HandleFunc("GET /api/backup/export", func(w http.ResponseWriter, r *http.Request) {
		includeSecrets := r.URL.Query().Get("secrets") == "1"
		backup := a.store.exportBackup(includeSecrets)
		filename := "devshell-backup-" + time.Now().Format("20060102-1504") + ".json"
		w.Header().Set("Content-Type", "application/json")
		w.Header().Set("Content-Disposition", `attachment; filename="`+filename+`"`)
		if err := json.NewEncoder(w).Encode(backup); err != nil {
			http.Error(w, err.Error(), http.StatusInternalServerError)
		}
	})
	mux.HandleFunc("POST /api/backup/import", func(w http.ResponseWriter, r *http.Request) {
		var backup Backup
		decoder := json.NewDecoder(io.LimitReader(r.Body, backupMaxBodyBytes+1))
		if err := decoder.Decode(&backup); err != nil {
			respond(w, nil, errors.New("备份文件解析失败"))
			return
		}
		if err := decoder.Decode(&json.RawMessage{}); !errors.Is(err, io.EOF) {
			respond(w, nil, errors.New("备份文件包含多余内容或超出大小限制"))
			return
		}
		summary, err := a.store.importBackup(backup)
		respond(w, summary, err)
	})
}
