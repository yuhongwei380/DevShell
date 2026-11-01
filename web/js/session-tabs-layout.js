// Keep navigation and the new-tab button outside the scrolling tab viewport.
(() => {
  const tabs = document.getElementById('session-tabs');
  const header = document.querySelector('.session-bar');
  const previous = document.getElementById('session-tabs-previous');
  const next = document.getElementById('session-tabs-next');
  const list = document.getElementById('session-tabs-list');
  const menu = document.getElementById('session-tabs-menu');
  let frame = 0, previousHeight = 0;
  function closeMenu() { menu.hidden = true; list.setAttribute('aria-expanded', 'false'); }
  function layout() {
    frame = 0;
    const maximum = tabs.scrollWidth - tabs.clientWidth;
    previous.disabled = tabs.scrollLeft <= 1;
    next.disabled = tabs.scrollLeft >= maximum - 1;
    list.disabled = !tabs.children.length;
    const height = header.offsetHeight;
    if (height !== previousHeight) {
      previousHeight = height;
      document.documentElement.style.setProperty('--titlebar-height', `${height}px`);
      if (typeof fitActive === 'function') fitActive();
      if (typeof $ === 'function' && typeof fitServerExplorer === 'function') fitServerExplorer();
    }
  }
  function schedule() {
    if (!frame) frame = requestAnimationFrame(layout);
  }
  function scroll(direction) {
    tabs.scrollBy({ left: direction * Math.max(100, tabs.clientWidth * .75), behavior: 'auto' });
    schedule();
  }
  previous.onclick = () => scroll(-1);
  next.onclick = () => scroll(1);
  list.onclick = () => {
    if (!menu.hidden) { closeMenu(); return; }
    menu.replaceChildren(...[...tabs.children].map(tab => {
      const source = tab.querySelector('[role="tab"]');
      const item = document.createElement('button'); item.type = 'button';
      item.setAttribute('role', 'menuitem');
      const selected = tab.classList.contains('active');
      item.setAttribute('aria-current', selected ? 'page' : 'false');
      const check = icon('check'); check.style.visibility = selected ? 'visible' : 'hidden';
      item.append(check, node('span', '', tab.querySelector('.session-tab-label')?.textContent || source.title));
      item.title = source.title;
      item.onclick = () => { closeMenu(); source.click(); };
      return item;
    }));
    menu.hidden = false;
    const rect = list.getBoundingClientRect(), top = rect.bottom / effectiveScale + 6;
    menu.style.left = Math.max(8, Math.min(rect.right / effectiveScale - menu.offsetWidth, logicalWidth() - menu.offsetWidth - 8)) + 'px';
    menu.style.top = top + 'px';
    menu.style.maxHeight = Math.min(360, Math.max(0, logicalHeight() - top - 8)) + 'px';
    list.setAttribute('aria-expanded', 'true');
    const selected = menu.querySelector('[aria-current="page"]') || menu.firstElementChild;
    selected?.scrollIntoView({ block: 'nearest' });
    selected?.focus({ preventScroll: true });
  };
  document.addEventListener('click', event => { if (!menu.contains(event.target) && !list.contains(event.target)) closeMenu(); }, true);
  document.addEventListener('keydown', event => {
    if (event.key === 'Escape' && !menu.hidden) { closeMenu(); list.focus(); }
    else if (!menu.hidden && menu.contains(event.target) && ['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
      event.preventDefault();
      const items = [...menu.children], index = items.indexOf(document.activeElement);
      items[event.key === 'Home' ? 0 : event.key === 'End' ? items.length - 1 : (index + (event.key === 'ArrowUp' ? -1 : 1) + items.length) % items.length]?.focus();
    }
  });
  tabs.addEventListener('scroll', schedule, { passive: true });
  tabs.addEventListener('wheel', event => {
    if (event.ctrlKey || event.metaKey || tabs.scrollWidth <= tabs.clientWidth || Math.abs(event.deltaX) >= Math.abs(event.deltaY)) return;
    event.preventDefault(); tabs.scrollLeft += event.deltaY;
  }, { passive: false });
  const sizeObserver = new ResizeObserver(() => { closeMenu(); schedule(); });
  sizeObserver.observe(tabs); sizeObserver.observe(header);
  new MutationObserver(() => { closeMenu(); schedule(); }).observe(tabs, { childList: true });
  window.addEventListener('resize', () => { closeMenu(); schedule(); });
  schedule();
})();
