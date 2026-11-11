/* iTerm2 color presets: XML/binary plist, legacy Generic RGB, sRGB and P3.
   Color-space decoding follows NSDictionary+iTerm; the Generic RGB primaries
   are Apple's profile (also documented by Chromium's skcolorspace_primaries.h).
   Convert to sRGB before handing colors to xterm's sRGB-only theme service. */
'use strict';
window.DevShellItermColors = (() => {
  const ansi = ['black', 'red', 'green', 'yellow', 'blue', 'magenta', 'cyan', 'white', 'brightBlack', 'brightRed', 'brightGreen', 'brightYellow', 'brightBlue', 'brightMagenta', 'brightCyan', 'brightWhite'];
  const labels = Object.fromEntries(ansi.map((key, index) => [key, `Ansi ${index} Color`]));
  Object.assign(labels, { foreground: 'Foreground Color', background: 'Background Color', bold: 'Bold Color', link: 'Link Color', match: 'Match Color', cursor: 'Cursor Color', cursorAccent: 'Cursor Text Color', selectionBackground: 'Selection Color', selectionForeground: 'Selected Text Color', underline: 'Underline Color', tab: 'Tab Color', cursorGuide: 'Cursor Guide Color', badge: 'Badge Color' });
  const clamp = value => Math.max(0, Math.min(1, value));
  const encode = value => value <= 0.0031308 ? 12.92 * value : 1.055 * value ** (1 / 2.4) - 0.055;
  const decode = value => value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
  const genericToSRGB = [[1.02499074, -0.02631958, 0.00132884], [0.01937382, 0.94803519, 0.032591], [-0.00178264, -0.00147009, 1.00325273]];
  const p3ToSRGB = [[1.22494018, -0.22494018, 0], [-0.04205695, 1.04205695, 0], [-0.01963755, -0.07863605, 1.0982736]];
  function component(value, label) {
    if ((typeof value !== 'number' && typeof value !== 'string') || String(value).trim() === '' || !Number.isFinite(Number(value))) throw new Error(`iTerm 配色「${label}」的颜色分量无效`);
    return clamp(Number(value));
  }
  function color(entry, label) {
    if (!entry || typeof entry !== 'object' || Array.isArray(entry)) throw new Error(`iTerm 配色「${label}」格式无效`);
    let rgb = ['Red Component', 'Green Component', 'Blue Component'].map(key => component(entry[key], label));
    const space = entry['Color Space'] || 'Calibrated';
    // iTerm2 treats an absent/legacy color-space name as calibrated Generic RGB.
    if (space !== 'sRGB') {
      const linear = rgb.map(value => space === 'P3' ? decode(value) : value ** 1.8);
      const matrix = space === 'P3' ? p3ToSRGB : genericToSRGB;
      rgb = matrix.map(row => encode(clamp(row.reduce((sum, weight, index) => sum + weight * linear[index], 0))));
    }
    const alpha = entry['Alpha Component'] === undefined ? (label.startsWith('Cursor Guide Color') ? 0.25 : 1) : component(entry['Alpha Component'], label);
    const hex = value => Math.round(clamp(value) * 255).toString(16).padStart(2, '0');
    return '#' + rgb.map(hex).join('') + (alpha < 1 ? hex(alpha) : '');
  }
  function xmlPlist(text) {
    const parsed = new DOMParser().parseFromString(text, 'application/xml');
    const root = parsed.documentElement;
    if (parsed.querySelector('parsererror') || root?.tagName !== 'plist' || root.children.length !== 1 || root.firstElementChild.tagName !== 'dict') throw new Error('不是有效的 iTerm 配色文件');
    function value(node, depth = 0) {
      if (depth > 32) throw new Error('iTerm 配色文件嵌套过深');
      switch (node.tagName) {
        case 'dict': {
          const result = Object.create(null), children = [...node.children];
          if (children.length % 2) throw new Error('iTerm 配色字典缺少值');
          for (let i = 0; i < children.length; i += 2) {
            const key = children[i];
            if (key.tagName !== 'key' || Object.hasOwn(result, key.textContent)) throw new Error('iTerm 配色字典的键无效或重复');
            result[key.textContent] = value(children[i + 1], depth + 1);
          }
          return result;
        }
        case 'array': return [...node.children].map(node => value(node, depth + 1));
        case 'real': case 'integer': return node.textContent.trim() ? Number(node.textContent) : NaN;
        case 'true': return true;
        case 'false': return false;
        case 'string': return node.textContent;
        default: return null;
      }
    }
    return value(root.firstElementChild);
  }
  // Bounded binary plist reader. Presets use dictionaries, strings and numbers;
  // other plist values are accepted as metadata and never executed.
  function binaryPlist(bytes) {
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength), end = bytes.length - 32;
    const invalid = () => { throw new Error('不是有效的 iTerm 二进制配色文件'); };
    const uint = (offset, size, limit = end) => {
      if (![1, 2, 4, 8].includes(size) || offset < 0 || offset + size > limit) return invalid();
      let value = 0;
      for (let i = 0; i < size; i++) value = value * 256 + bytes[offset + i];
      if (!Number.isSafeInteger(value)) return invalid();
      return value;
    };
    if (end < 8) return invalid();
    const offsetSize = bytes[end + 6], refSize = bytes[end + 7];
    const count = uint(end + 8, 8, bytes.length), top = uint(end + 16, 8, bytes.length), table = uint(end + 24, 8, bytes.length);
    if (!count || count > bytes.length || top >= count || table < 8 || table + count * offsetSize !== end) return invalid();
    const offsets = Array.from({ length: count }, (_, i) => uint(table + i * offsetSize, offsetSize));
    const active = new Set(), cache = new Map();
    function read(ref, depth = 0) {
      if (ref >= count || depth > 32 || active.has(ref)) return invalid();
      if (cache.has(ref)) return cache.get(ref);
      active.add(ref);
      let pos = offsets[ref];
      if (pos < 8 || pos >= table) return invalid();
      const marker = bytes[pos++], type = marker >> 4;
      let length = marker & 15, result;
      if ([4, 5, 6, 10, 13].includes(type) && length === 15) {
        const integer = bytes[pos++];
        if (integer >> 4 !== 1) return invalid();
        const size = 2 ** (integer & 15);
        length = uint(pos, size, table); pos += size;
      }
      const requireBytes = size => { if (!Number.isSafeInteger(size) || size < 0 || pos + size > table) invalid(); };
      switch (type) {
        case 0: result = length === 8 ? false : length === 9 ? true : null; break;
        case 1: {
          const size = 2 ** length; requireBytes(size);
          result = size === 8 ? Number(view.getBigInt64(pos)) : uint(pos, size, table);
          if (!Number.isSafeInteger(result)) return invalid();
          break;
        }
        case 2: {
          const size = 2 ** length; requireBytes(size);
          if (size !== 4 && size !== 8) return invalid();
          result = size === 4 ? view.getFloat32(pos) : view.getFloat64(pos); break;
        }
        case 4: requireBytes(length); result = null; break;
        case 3: requireBytes(8); result = null; break; // Metadata date.
        case 8: requireBytes(length + 1); result = null; break; // Metadata UID.
        case 5: case 6: {
          requireBytes(length * (type === 6 ? 2 : 1));
          result = new TextDecoder(type === 6 ? 'utf-16be' : 'utf-8', { fatal: true }).decode(bytes.subarray(pos, pos + length * (type === 6 ? 2 : 1))); break;
        }
        case 10: case 13: {
          requireBytes(length * refSize * (type === 13 ? 2 : 1));
          const refs = Array.from({ length: length }, (_, i) => uint(pos + i * refSize, refSize, table));
          if (type === 10) result = refs.map(ref => read(ref, depth + 1));
          else {
            result = Object.create(null);
            refs.forEach((ref, i) => {
              const key = read(ref, depth + 1);
              if (typeof key !== 'string' || Object.hasOwn(result, key)) invalid();
              result[key] = read(uint(pos + (length + i) * refSize, refSize, table), depth + 1);
            });
          }
          break;
        }
        default: return invalid();
      }
      active.delete(ref); cache.set(ref, result); return result;
    }
    return read(top);
  }
  const defaultAnsi = ['000000', 'bb0000', '00bb00', 'bbbb00', '0000bb', 'bb00bb', '00bbbb', 'bbbbbb', '555555', 'ff5555', '55ff55', 'ffff55', '5555ff', '0000bb', '55ffff', 'ffffff'];
  function defaults() {
    const result = {};
    const convert = (hex, label) => color(Object.fromEntries(['Red Component', 'Green Component', 'Blue Component'].map((key, i) => [key, parseInt(hex.slice(i * 2, i * 2 + 2), 16) / 255])), label);
    ansi.forEach((key, i) => { result[key] = convert(defaultAnsi[i], labels[key]); });
    for (const [key, hex] of Object.entries({ foreground: 'bbbbbb', background: '000000', cursor: 'bbbbbb', cursorAccent: 'ffffff', selectionBackground: 'b5d5ff', selectionForeground: '000000', bold: 'ffffff' })) result[key] = convert(hex, labels[key]);
    return result;
  }
  function parse(data) {
    const bytes = typeof data === 'string' ? null : new Uint8Array(data);
    if (bytes && bytes.length > 256 * 1024) throw new Error('iTerm 配色文件不能超过 256 KiB');
    const binary = bytes && new TextDecoder().decode(bytes.subarray(0, 8)) === 'bplist00';
    const top = binary ? binaryPlist(bytes) : xmlPlist(typeof data === 'string' ? data : new TextDecoder(bytes[0] === 0xff && bytes[1] === 0xfe ? 'utf-16le' : bytes[0] === 0xfe && bytes[1] === 0xff ? 'utf-16be' : 'utf-8', { fatal: true }).decode(bytes));
    if (!top || typeof top !== 'object' || Array.isArray(top)) throw new Error('不是有效的 iTerm 配色文件');
    let found = false;
    function palette(suffix, base) {
      const result = { ...base };
      for (const [key, label] of Object.entries(labels)) {
        if (!Object.hasOwn(top, label + suffix)) continue;
        found = true; result[key] = color(top[label + suffix], label + suffix);
      }
      return result;
    }
    const colors = palette('', defaults()), result = { colors };
    if (Object.keys(top).some(key => key.endsWith(' (Light)') && Object.values(labels).includes(key.slice(0, -8)))) result.lightColors = palette(' (Light)', colors);
    if (Object.keys(top).some(key => key.endsWith(' (Dark)') && Object.values(labels).includes(key.slice(0, -7)))) result.darkColors = palette(' (Dark)', colors);
    if (!found) throw new Error('文件中没有 iTerm 主题颜色');
    return result;
  }
  return { parse };
})();
