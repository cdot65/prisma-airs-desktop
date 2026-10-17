// C01 native acceptance driver (JXA, run with `osascript -l JavaScript`).
//
// Drives the real fixture window through macOS accessibility (System Events):
// reads the AX tree and menus, sends keystrokes, resizes the window and
// records an AX snapshot for every step. It needs the Accessibility grant for
// the invoking process. It cannot see pixels: visual properties (focus-ring
// rendering, theme colors, motion) are reported as not observable.
//
// Usage: osascript -l JavaScript c01_ax_driver.js <pid>
// Output: JSON {steps:[{id, action, observation, snapshot}], menus, ...}

function run(argv) {
  const pid = parseInt(argv[0], 10);
  const se = Application('System Events');
  const proc = se.applicationProcesses.whose({ unixId: pid })[0];
  const steps = [];

  function attr(el, name) { try { return el.attributes.byName(name).value(); } catch (e) { return null; } }
  function front() { proc.frontmost = true; delay(0.4); }
  function win() { return proc.windows[0]; }
  function label(el) {
    if (!el) return null;
    return attr(el, 'AXTitle') || attr(el, 'AXDescription') ||
      (function () { const v = attr(el, 'AXValue'); return v === null || typeof v === 'object' ? null : String(v); })();
  }
  function focused() {
    const el = attr(proc, 'AXFocusedUIElement');
    if (!el) return null;
    return { role: attr(el, 'AXRole'), label: label(el), current: attr(el, 'AXARIACurrent') };
  }
  function tree() {
    const out = [];
    function walk(el, depth) {
      if (depth > 40 || out.length > 800) return;
      out.push({ depth: depth, role: attr(el, 'AXRole'), subrole: attr(el, 'AXSubrole'), label: label(el),
        value: (function () { const v = attr(el, 'AXValue'); return v === null || typeof v === 'object' ? null : v; })(),
        focused: attr(el, 'AXFocused'), current: attr(el, 'AXARIACurrent'), selected: attr(el, 'AXSelected'),
        pos: attr(el, 'AXPosition'), size: attr(el, 'AXSize') });
      let kids = []; try { kids = el.uiElements(); } catch (e) {}
      for (const k of kids) walk(k, depth + 1);
    }
    walk(win(), 0);
    return out;
  }
  function find(nodes, pred) { return nodes.filter(pred); }
  function key(code, mods) { front(); se.keyCode(code, mods ? { using: mods } : {}); delay(0.5); }
  function stroke(ch, mods) { front(); se.keystroke(ch, mods ? { using: mods } : {}); delay(0.6); }
  // A radio group is one Tab stop that lands on its checked radio, and WebKit
  // does not wrap arrow keys at the ends. Tab into the group, move with Up or
  // Down Arrow (by tree order) to the radio whose label starts with `name`,
  // press Space, and report whether it is checked afterwards plus its frame
  // relative to the window and the keys used.
  function operateRadio(name) {
    const keys = [];
    const isRadio = (el) => el && attr(el, 'AXRole') === 'AXRadioButton';
    let el = null;
    for (let i = 0; i < 60 && !isRadio(el); i++) { key(48); keys.push('Tab'); el = attr(proc, 'AXFocusedUIElement'); }
    if (!isRadio(el)) return null;
    const order = find(tree(), n => n.role === 'AXRadioButton').map(n => n.label || '');
    const at = (l) => order.findIndex(o => o === l);
    const target = order.findIndex(o => o.indexOf(name) === 0);
    if (target < 0 || at(label(el) || '') < 0) return null;
    const down = target > at(label(el) || '');
    for (let i = 0; i < order.length && (label(el) || '').indexOf(name) !== 0; i++) {
      key(down ? 125 : 126); keys.push(down ? 'Down' : 'Up');
      el = attr(proc, 'AXFocusedUIElement');
      if (!isRadio(el)) return null;
    }
    if ((label(el) || '').indexOf(name) !== 0) return null;
    key(49); keys.push('Space');
    delay(0.6);
    const after = attr(proc, 'AXFocusedUIElement');
    return { label: label(after), checked: attr(after, 'AXValue'), pos: attr(after, 'AXPosition'), size: attr(after, 'AXSize'),
      window: { pos: attr(win(), 'AXPosition'), size: attr(win(), 'AXSize') }, keys: keys };
  }
  function record(id, action, observation, snapshot) { steps.push({ id: id, action: action, observation: observation, snapshot: snapshot }); }
  // Fixture-only self-capture (Fixture > Capture Window); captures are
  // written in order and mapped to these names by the caller.
  const captures = [];
  function capture(name) {
    front();
    try { proc.menuBars[0].menuBarItems.byName('Fixture').menus[0].menuItems.byName('Capture Window').click(); captures.push(name); } catch (e) { captures.push('FAILED:' + name); }
    delay(1.2);
  }

  front();
  // Menus (names, items, shortcuts).
  const menus = [];
  const bar = proc.menuBars[0];
  for (const item of bar.menuBarItems()) {
    const name = item.name();
    const entries = [];
    try {
      for (const mi of item.menus[0].menuItems()) {
        const t = mi.name();
        if (!t) continue;
        entries.push({ title: t, cmdChar: attr(mi, 'AXMenuItemCmdChar'), modifiers: attr(mi, 'AXMenuItemCmdModifiers') });
      }
    } catch (e) {}
    menus.push({ menu: name, items: entries });
  }
  record('menus', 'read menu bar via AX', 'menu bar enumerated', null);

  capture('launch');
  let snap = tree();
  record('launch', 'read window and tree after launch', { title: attr(win(), 'AXTitle'), size: attr(win(), 'AXSize') }, snap);

  // Keyboard-only navigation: Tab until the Academy destination has focus.
  const tabs = [];
  for (let i = 0; i < 30; i++) {
    key(48);
    const f = focused();
    tabs.push(f);
    if (f && f.label && f.label.indexOf('Academy') === 0) break;
  }
  record('tab-order', 'Tab repeatedly from launch focus', tabs, null);
  capture('focus-ring-academy');
  key(36); // Return
  record('enter-academy', 'Return on the Academy destination', { focused: focused() }, tree());

  // Palette via Cmd+K, filter, Return; then Escape restores focus.
  const dialog = (nodes) => find(nodes, n => n.label === 'Command palette').length > 0;
  stroke('k', ['command down']);
  delay(0.8);
  const openTree = tree();
  const afterOpen = focused();
  stroke('gate');
  capture('palette-filtered');
  const paletteTree = tree();
  const options = find(paletteTree, n => n.role === 'AXStaticText' && n.depth > 6).map(n => n.label);
  key(36);
  record('palette', 'Cmd+K, type "gate", Return', { dialogOpenAfterCmdK: dialog(openTree), focusAfterOpen: afterOpen,
    optionsAfterTyping: options, dialogOpenAfterReturn: dialog(tree()), focusAfterReturn: focused() }, paletteTree);
  // Escape restores focus to the control focused before opening: use the trigger button.
  key(48, ['shift down']);
  const before = focused();
  stroke('k', ['command down']);
  delay(0.6);
  const openedAgain = dialog(tree());
  key(53); // Escape
  delay(0.4);
  record('palette-escape', 'focus a control, Cmd+K, Escape', { before: before, dialogOpened: openedAgain, dialogOpenAfterEscape: dialog(tree()), after: focused() }, null);

  // Every destination through the Go menu shortcuts.
  const destinations = [];
  for (let n = 1; n <= 9; n++) {
    stroke(String(n), ['command down']);
    const t = tree();
    const h = find(t, x => x.role === 'AXHeading');
    destinations.push({ shortcut: 'Cmd+' + n, heading: h.length ? h[0].label : null,
      unavailableText: find(t, x => x.label && x.label.indexOf('is not available in this build') >= 0).map(x => x.label),
      planned: find(t, x => x.label && x.label.indexOf('PLANNED IN') === 0).map(x => x.label) });
  }
  front();
  try { bar.menuBarItems.byName('Go').menus[0].menuItems.byName('Import from v1').click(); } catch (e) {}
  delay(0.6);
  const ti = tree();
  destinations.push({ shortcut: 'Go > Import from v1', heading: (find(ti, x => x.role === 'AXHeading')[0] || {}).label || null,
    unavailableText: find(ti, x => x.label && x.label.indexOf('is not available in this build') >= 0).map(x => x.label),
    planned: find(ti, x => x.label && x.label.indexOf('PLANNED IN') === 0).map(x => x.label) });
  record('destinations', 'Cmd+1..Cmd+9 and Go > Import from v1', destinations, null);

  // Native Go shortcuts, including while the palette is open.
  stroke('2', ['command down']);
  record('go-academy', 'Cmd+2', { focused: focused() }, null);
  stroke('k', ['command down']);
  stroke(',', ['command down']);
  const settingsTree = tree();
  record('go-settings-with-palette-open', 'Cmd+K then Cmd+,', { focused: focused(),
    dialogOpen: find(settingsTree, n => n.role === 'AXGroup' && n.label === 'Command palette').length > 0 }, settingsTree);

  // Appearance through Settings radios and through the View menu.
  const radios = () => find(tree(), n => n.role === 'AXRadioButton').map(n => ({ label: n.label, checked: n.value }));
  record('appearance-initial', 'read appearance radios', radios(), null);
  front();
  try { bar.menuBarItems.byName('View').menus[0].menuItems.byName('Appearance').menus[0].menuItems.byName('Dark').click(); } catch (e) {}
  delay(0.8);
  record('appearance-view-dark', 'View > Appearance > Dark', radios(), tree());
  capture('settings-dark');
  front();
  try { bar.menuBarItems.byName('View').menus[0].menuItems.byName('Appearance').menus[0].menuItems.byName('Light').click(); } catch (e) {}
  delay(0.8);
  record('appearance-view-light', 'View > Appearance > Light', radios(), null);
  capture('settings-light');

  // Zoom: measure the heading before/after Cmd+= twice and reset.
  const heading = () => { const h = find(tree(), n => n.role === 'AXHeading'); return h.length ? { label: h[0].label, size: h[0].size } : null; };
  const z0 = heading();
  stroke('=', ['command down']); stroke('=', ['command down']); stroke('=', ['command down']); stroke('=', ['command down']); stroke('=', ['command down']);
  const z150 = heading();
  for (let i = 0; i < 5; i++) stroke('=', ['command down']);
  const z200 = heading();
  capture('zoom-200');
  const zoomTree = tree();
  // At 200%, keyboard focus must be able to reach the last destination and
  // it must scroll into the visible window.
  let reached = null;
  for (let i = 0; i < 40; i++) {
    key(48);
    const f = focused();
    if (f && f.label === 'Settings' && f.role === 'AXButton') {
      const el = attr(proc, 'AXFocusedUIElement');
      reached = { label: f.label, pos: attr(el, 'AXPosition'), size: attr(el, 'AXSize'), window: { pos: attr(win(), 'AXPosition'), size: attr(win(), 'AXSize') } };
      break;
    }
  }
  const radioAt200 = operateRadio('Dark');
  capture('zoom-200-settings');
  stroke('0', ['command down']);
  record('zoom', 'Cmd+= x5 (150%), x10 (200%), Tab to the last destination, Cmd+0', { base: z0, at150: z150, at200: z200, lastDestinationAt200: reached, settingsRadioAt200: radioAt200, reset: heading() }, zoomTree);

  // Minimum window size.
  front();
  try { win().size = [900, 600]; } catch (e) {}
  delay(0.8);
  capture('min-size');
  const minTree = tree();
  let minReached = null;
  for (let i = 0; i < 40; i++) {
    key(48);
    const f = focused();
    if (f && f.label === 'Settings' && f.role === 'AXButton') {
      const el = attr(proc, 'AXFocusedUIElement');
      minReached = { pos: attr(el, 'AXPosition'), size: attr(el, 'AXSize'), window: { pos: attr(win(), 'AXPosition'), size: attr(win(), 'AXSize') } };
      break;
    }
  }
  const radioAtMin = operateRadio('Light');
  capture('min-size-settings');
  record('min-size', 'resize window to 900x600, Tab to the last destination, operate the Light radio', { size: attr(win(), 'AXSize'), lastDestination: minReached, settingsRadio: radioAtMin }, minTree);
  try { win().size = [1280, 820]; } catch (e) {}
  delay(0.5);

  return JSON.stringify({ pid: pid, menus: menus, steps: steps, captures: captures }, null, 1);
}
