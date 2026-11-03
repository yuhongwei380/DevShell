package app

import (
	"errors"
	"fmt"
	"strings"
	"unicode/utf8"
)

type ServerGroup struct {
	ID              string `json:"id"`
	Name            string `json:"name"`
	ParentID        string `json:"parentId"`
	Emoji           string `json:"emoji"`
	BackgroundColor string `json:"backgroundColor,omitempty"`
	Order           int    `json:"order,omitempty"`
}

func groupIndex(groups []ServerGroup) map[string]ServerGroup {
	result := make(map[string]ServerGroup, len(groups))
	for _, group := range groups {
		result[group.ID] = group
	}
	return result
}
func validateGroupGraph(groups []ServerGroup) error {
	byID := make(map[string]ServerGroup, len(groups))
	siblings := map[string]map[string]bool{}
	for _, group := range groups {
		if group.ID == "" || group.Name == "" {
			return errors.New("分组 ID 或名称为空")
		}
		if err := validateGroupInput(group); err != nil {
			return err
		}
		if _, exists := byID[group.ID]; exists {
			return errors.New("分组 ID 重复")
		}
		byID[group.ID] = group
		if siblings[group.ParentID] == nil {
			siblings[group.ParentID] = map[string]bool{}
		}
		if siblings[group.ParentID][group.Name] {
			return fmt.Errorf("同一层级中分组「%s」重复", group.Name)
		}
		siblings[group.ParentID][group.Name] = true
	}
	for _, group := range groups {
		if group.ParentID != "" {
			if _, exists := byID[group.ParentID]; !exists {
				return errors.New("父分组不存在")
			}
		}
	}
	// Iterative traversal imposes no hierarchy-depth limit and does not consume
	// call-stack depth. Every node becomes complete once, including deep chains.
	done := map[string]bool{}
	for _, group := range groups {
		chain := []string{}
		visiting := map[string]bool{}
		id := group.ID
		for id != "" && !done[id] {
			if visiting[id] {
				return errors.New("不能将分组移到自身或其子分组中")
			}
			visiting[id] = true
			chain = append(chain, id)
			id = byID[id].ParentID
		}
		for _, id := range chain {
			done[id] = true
		}
	}
	return nil
}
func legacyGroupPath(id string, groups map[string]ServerGroup) string {
	parts := []string{}
	for id != "" {
		group, ok := groups[id]
		if !ok {
			break
		}
		parts = append(parts, group.Name)
		id = group.ParentID
	}
	for i, j := 0, len(parts)-1; i < j; i, j = i+1, j-1 {
		parts[i], parts[j] = parts[j], parts[i]
	}
	return strings.Join(parts, " / ")
}
func (s *Store) refreshLegacyGroupsLocked() {
	groups := groupIndex(s.config.GroupNodes)
	s.config.Groups = make([]string, 0, len(s.config.GroupNodes))
	paths := map[string]string{}
	for _, group := range s.config.GroupNodes {
		value := legacyGroupPath(group.ID, groups)
		paths[group.ID] = value
		s.config.Groups = append(s.config.Groups, value)
	}
	for i, profile := range s.config.Servers {
		s.config.Servers[i].Group = paths[profile.GroupID]
	}
}
func (s *Store) assignProfileGroupLocked(profile *Profile) error {
	if profile.GroupID != "" {
		for _, group := range s.config.GroupNodes {
			if group.ID == profile.GroupID {
				profile.Group = legacyGroupPath(group.ID, groupIndex(s.config.GroupNodes))
				return nil
			}
		}
		return errors.New("所选分组不存在，请重新选择")
	}
	name := strings.TrimSpace(profile.Group)
	if name == "" {
		name = "我的服务器"
	}
	groups := groupIndex(s.config.GroupNodes)
	for _, group := range s.config.GroupNodes {
		if legacyGroupPath(group.ID, groups) == name {
			if profile.GroupID != "" {
				return errors.New("旧版分组名称存在歧义，请按分组 ID 重新选择")
			}
			profile.GroupID = group.ID
		}
	}
	if profile.GroupID == "" {
		if len(name) > 100 {
			return errors.New("分组名称过长")
		}
		group := ServerGroup{ID: randomID(), Name: name}
		s.config.GroupNodes = append(s.config.GroupNodes, group)
		profile.GroupID = group.ID
	}
	profile.Group = name
	return nil
}
func validateGroupInput(group ServerGroup) error {
	if group.Order < 0 {
		return errors.New("分组顺序不能为负数")
	}
	if group.BackgroundColor != "" {
		if len(group.BackgroundColor) != 7 || group.BackgroundColor[0] != '#' || strings.Trim(group.BackgroundColor[1:], "0123456789abcdefABCDEF") != "" {
			return errors.New("分组背景色必须是六位十六进制颜色")
		}
	}
	if group.Name == "" || len(group.Name) > 100 || strings.ContainsAny(group.Name, "\x00\r\n") {
		return errors.New("请填写有效分组名称（最多 100 字节）")
	}
	// Emoji may contain joiners, variation selectors, flags and skin modifiers.
	// Limit only its UTF-8 size; never split a multi-codepoint emoji sequence.
	if !utf8.ValidString(group.Emoji) || len(group.Emoji) > 128 || strings.ContainsAny(group.Emoji, "\x00\r\n") {
		return errors.New("分组图标无效或过长")
	}
	return nil
}
func (s *Store) saveGroupLocked(group ServerGroup) (ServerGroup, error) {
	group.Name = strings.TrimSpace(group.Name)
	group.Emoji = strings.TrimSpace(group.Emoji)
	if err := validateGroupInput(group); err != nil {
		return ServerGroup{}, err
	}
	old := cloneConnectionConfig(s.config)
	index := -1
	for i, current := range s.config.GroupNodes {
		if current.ID == group.ID {
			index = i
			break
		}
	}
	if group.ID == "" {
		group.ID = randomID()
	} else if index < 0 {
		return ServerGroup{}, errors.New("分组不存在")
	}
	// Only the reorder operation changes sibling order. Editing keeps it;
	// new groups and groups moved to another parent follow ordered siblings.
	group.Order = 0
	if index >= 0 && s.config.GroupNodes[index].ParentID == group.ParentID {
		group.Order = s.config.GroupNodes[index].Order
	}
	next := append([]ServerGroup{}, s.config.GroupNodes...)
	if index < 0 {
		next = append(next, group)
	} else {
		next[index] = group
	}
	if err := validateGroupGraph(next); err != nil {
		return ServerGroup{}, err
	}
	s.config.GroupNodes = next
	s.refreshLegacyGroupsLocked()
	if err := s.writeLocked(); err != nil {
		s.config = old
		return ServerGroup{}, err
	}
	return group, nil
}
func (s *Store) SaveGroup(group ServerGroup) (ServerGroup, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.saveGroupLocked(group)
}

// Reorder a complete sibling set under the store lock, rejecting stale lists
// rather than dropping a concurrently created or moved group.
func (s *Store) ReorderGroups(parentID string, ids []string) ([]ServerGroup, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	byID := groupIndex(s.config.GroupNodes)
	if parentID != "" {
		if _, ok := byID[parentID]; !ok {
			return nil, errors.New("父分组不存在，请刷新后重试")
		}
	}
	count := 0
	for _, group := range s.config.GroupNodes {
		if group.ParentID == parentID {
			count++
		}
	}
	if len(ids) != count || count == 0 {
		return nil, errors.New("分组列表已变化，请刷新后重试")
	}
	positions := make(map[string]int, count)
	for i, id := range ids {
		group, ok := byID[id]
		if !ok || group.ParentID != parentID || positions[id] != 0 {
			return nil, errors.New("只能对同一层级的全部分组排序，且不能重复")
		}
		positions[id] = i + 1
	}
	old := cloneConnectionConfig(s.config)
	for i, group := range s.config.GroupNodes {
		if order := positions[group.ID]; order != 0 {
			s.config.GroupNodes[i].Order = order
		}
	}
	if err := s.writeLocked(); err != nil {
		s.config = old
		return nil, err
	}
	return append([]ServerGroup{}, s.config.GroupNodes...), nil
}

// Move only the group fields, preserving the latest connection and credentials.
func (s *Store) MoveProfileGroup(id, groupID string) (Profile, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	groups := groupIndex(s.config.GroupNodes)
	if _, ok := groups[groupID]; !ok {
		return Profile{}, errors.New("目标分组不存在")
	}
	for i, profile := range s.config.Servers {
		if profile.ID != id || profile.DeletedAt != nil {
			continue
		}
		if profile.GroupID == groupID {
			return publicProfile(profile), nil
		}
		s.config.Servers[i].GroupID = groupID
		s.config.Servers[i].Group = legacyGroupPath(groupID, groups)
		if err := s.writeLocked(); err != nil {
			s.config.Servers[i] = profile
			return Profile{}, err
		}
		return publicProfile(s.config.Servers[i]), nil
	}
	return Profile{}, errors.New("服务器配置不存在或已删除")
}

func (s *Store) deleteGroupLocked(id string) error {
	found := false
	for _, group := range s.config.GroupNodes {
		if group.ID == id {
			found = true
		}
		if group.ParentID == id {
			return errors.New("请先移动或删除该分组的子分组")
		}
	}
	if !found {
		return errors.New("分组不存在")
	}
	for _, profile := range s.config.Servers {
		if profile.GroupID == id {
			if profile.DeletedAt != nil {
				return fmt.Errorf("回收站中的服务器「%s」仍属于此分组，请先恢复并移动或永久删除", profile.Name)
			}
			return errors.New("请先移动或删除分组内的服务器")
		}
	}
	old := cloneConnectionConfig(s.config)
	next := []ServerGroup{}
	for _, group := range s.config.GroupNodes {
		if group.ID != id {
			next = append(next, group)
		}
	}
	s.config.GroupNodes = next
	s.refreshLegacyGroupsLocked()
	if err := s.writeLocked(); err != nil {
		s.config = old
		return err
	}
	return nil
}
func (s *Store) DeleteGroup(id string) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.deleteGroupLocked(id)
}

// DeleteGroupKeepConnections removes just this group node. Its connections
// (including trashed ones) and direct child groups relocate into targetID,
// defaulting to the deleted group's parent. The store's invariant is that
// every profile belongs to a real group — there is no groupless top level —
// so the target must exist and must not be inside the deleted subtree.
// Returns how many connections were relocated.
func (s *Store) DeleteGroupKeepConnections(id, targetID string) (int, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	byID := groupIndex(s.config.GroupNodes)
	group, found := byID[id]
	if !found {
		return 0, errors.New("分组不存在")
	}
	target := strings.TrimSpace(targetID)
	if target == "" {
		target = group.ParentID
	}
	if target == id {
		return 0, errors.New("不能移入正在删除的分组")
	}
	if _, ok := byID[target]; !ok {
		return 0, errors.New("接管的分组不存在，请先创建其他分组")
	}
	children := map[string][]string{}
	for _, entry := range s.config.GroupNodes {
		children[entry.ParentID] = append(children[entry.ParentID], entry.ID)
	}
	subtree := map[string]bool{id: true}
	queue := []string{id}
	for i := 0; i < len(queue); i++ {
		for _, child := range children[queue[i]] {
			if !subtree[child] {
				subtree[child] = true
				queue = append(queue, child)
			}
		}
	}
	if subtree[target] {
		return 0, errors.New("接管分组位于被删除分组内部，请选择其外的分组")
	}
	old := cloneConnectionConfig(s.config)
	next := []ServerGroup{}
	for _, entry := range s.config.GroupNodes {
		if entry.ID == id {
			continue
		}
		if entry.ParentID == id {
			entry.ParentID = target
		}
		next = append(next, entry)
	}
	s.config.GroupNodes = next
	moved := 0
	for i := range s.config.Servers {
		if s.config.Servers[i].GroupID == id {
			s.config.Servers[i].GroupID = target
			moved++
		}
	}
	s.refreshLegacyGroupsLocked()
	if err := s.writeLocked(); err != nil {
		s.config = old
		return 0, err
	}
	return moved, nil
}

// Legacy clients still send flat display names. Resolve a unique display path
// and delegate to ID-based operations; never silently pick a name collision.
func (s *Store) Group(action, name, newName string) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	name = strings.TrimSpace(name)
	newName = strings.TrimSpace(newName)
	if name == "" || len(name) > 100 || len(newName) > 100 {
		return errors.New("请填写有效分组名称")
	}
	var match *ServerGroup
	byID := groupIndex(s.config.GroupNodes)
	for _, group := range s.config.GroupNodes {
		if legacyGroupPath(group.ID, byID) == name {
			if match != nil {
				return errors.New("分组名称存在歧义，请使用新版分组管理")
			}
			copy := group
			match = &copy
		}
	}
	switch action {
	case "add":
		if match != nil {
			return errors.New("分组已存在")
		}
		_, err := s.saveGroupLocked(ServerGroup{Name: name})
		return err
	case "rename":
		if match == nil || newName == "" {
			return errors.New("分组不存在或新名称为空")
		}
		match.Name = newName
		_, err := s.saveGroupLocked(*match)
		return err
	case "delete":
		if match == nil {
			return errors.New("分组不存在")
		}
		return s.deleteGroupLocked(match.ID)
	default:
		return errors.New("未知分组操作")
	}
}
