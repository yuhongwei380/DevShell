'use strict';
// Apply the same category emphasis to plain-text update offers and rendered logs.
window.DengReleaseNoteTags = (() => {
  const kinds = { 新增: 'added', 修复: 'fixed', 调整: 'changed', 安全: 'security' };
  function highlight(container) {
    const walker = document.createTreeWalker(container, NodeFilter.SHOW_TEXT), texts = [];
    while (walker.nextNode()) {
      if (!walker.currentNode.parentElement.closest('pre,code,.release-note-tag')) texts.push(walker.currentNode);
    }
    for (const text of texts) {
      const matches = [...text.data.matchAll(/(^|[\r\n])([ \t]*(?:(?:[-*+]|\d+[.)])[ \t]+)?)(新增|修复|调整|安全)([：:])([ \t]*)/g)];
      if (!matches.length) continue;
      const fragment = document.createDocumentFragment();
      let offset = 0;
      for (const match of matches) {
        const start = match.index + match[1].length;
        fragment.append(document.createTextNode(text.data.slice(offset, start)));
        const marker = document.createElement('span'); marker.className = 'release-note-marker';
        marker.textContent = match[2]; fragment.append(marker);
        const tag = document.createElement('span');
        tag.className = 'release-note-tag'; tag.dataset.kind = kinds[match[3]];
        tag.textContent = match[3] + match[4]; fragment.append(tag);
        const spacing = document.createElement('span'); spacing.className = 'release-note-marker';
        spacing.textContent = match[5]; fragment.append(spacing);
        offset = match.index + match[0].length;
      }
      fragment.append(document.createTextNode(text.data.slice(offset)));
      text.replaceWith(fragment);
    }
    for (const tag of container.querySelectorAll('.release-note-tag')) tag.closest('li')?.classList.add('release-note-item');
    for (const list of container.querySelectorAll('ul,ol')) {
      list.classList.toggle('release-note-list', [...list.children].every(item => item.classList.contains('release-note-item')));
    }
  }
  return { highlight };
})();
