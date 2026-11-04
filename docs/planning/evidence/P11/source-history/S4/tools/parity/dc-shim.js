// A static renderer for the design prototype's component format (the
// Claude Design `.dc.html` files under docs/architecture/ui-v2/prototype/),
// enough to draw a board for a parity composite: the component's
// `renderVals()` is evaluated once with its props and an empty state, and the
// template is expanded. Event handlers are dropped (nothing is interactive),
// `setState` after the first render is ignored, and timers never fire.
//
// Template features honoured, as the frozen files use them:
//   {{path}} in text and attributes (a dotted path into the render values),
//   <sc-if value="{{path}}">…</sc-if>, <sc-for list="{{path}}" as="x">…</sc-for>,
//   <dc-import name="Other" attr="…"> (another component, attributes as props),
//   <helmet> (its <style> is hoisted into <head>, interpolated).
// Anything else is kept as plain markup.
(function () {
  'use strict';

  class DCLogic {
    constructor(props) {
      this.props = props || {};
      this.state = {};
    }
    setState(patch) {
      // The first render reads an empty state; later patches are not re-rendered.
      this.state = Object.assign({}, this.state, patch);
    }
  }
  window.DCLogic = DCLogic;

  const PATH = /^[A-Za-z_$][\w$]*(?:\.[A-Za-z_$][\w$]*)*$/;

  function lookup(scopes, expr) {
    const text = expr.trim();
    if (text === 'true') return true;
    if (text === 'false') return false;
    if (text === 'null') return null;
    if (/^-?\d+(\.\d+)?$/.test(text)) return Number(text);
    if (PATH.test(text)) {
      const parts = text.split('.');
      for (let i = scopes.length - 1; i >= 0; i--) {
        const scope = scopes[i];
        if (scope && Object.prototype.hasOwnProperty.call(scope, parts[0])) {
          let value = scope;
          for (const part of parts) {
            if (value === null || value === undefined) return undefined;
            value = value[part];
          }
          return value;
        }
      }
      return undefined;
    }
    // A JavaScript expression over the scopes (rare in the frozen files).
    try {
      const names = [];
      const values = [];
      for (const scope of scopes) {
        for (const key of Object.keys(scope || {})) {
          const at = names.indexOf(key);
          if (at >= 0) values[at] = scope[key];
          else {
            names.push(key);
            values.push(scope[key]);
          }
        }
      }
      return new Function(...names, 'return (' + text + ');')(...values);
    } catch (e) {
      return undefined;
    }
  }

  function interpolate(scopes, text) {
    return text.replace(/\{\{([^}]*)\}\}/g, (_, expr) => {
      const value = lookup(scopes, expr);
      return value === undefined || value === null ? '' : String(value);
    });
  }

  function isHandler(name) {
    return /^on[A-Z]/.test(name) || /^on[a-z]+$/.test(name);
  }

  // Expands a template subtree into `target` under the given scopes.
  function expand(node, scopes, target, components) {
    for (const child of Array.from(node.childNodes)) {
      if (child.nodeType === Node.TEXT_NODE) {
        target.appendChild(document.createTextNode(interpolate(scopes, child.data)));
        continue;
      }
      if (child.nodeType !== Node.ELEMENT_NODE) continue;
      const tag = child.tagName.toLowerCase();
      if (tag === 'sc-if') {
        const value = lookup(scopes, (child.getAttribute('value') || '').replace(/^\{\{|\}\}$/g, ''));
        if (value) expand(child, scopes, target, components);
        continue;
      }
      if (tag === 'sc-for') {
        const list = lookup(scopes, (child.getAttribute('list') || '').replace(/^\{\{|\}\}$/g, ''));
        const name = child.getAttribute('as') || 'item';
        if (Array.isArray(list)) {
          list.forEach((item, index) => {
            const scope = {};
            scope[name] = item;
            scope[name + 'Index'] = index;
            expand(child, scopes.concat([scope]), target, components);
          });
        }
        continue;
      }
      if (tag === 'dc-import') {
        const name = child.getAttribute('name');
        const props = {};
        for (const attr of Array.from(child.attributes)) {
          if (attr.name === 'name' || attr.name.startsWith('hint-')) continue;
          props[attr.name] = interpolate(scopes, attr.value);
        }
        const component = components[name];
        if (component) {
          const host = document.createElement('div');
          host.setAttribute('data-dc-import', name);
          render(component, props, host, components);
          target.appendChild(host);
        } else {
          const missing = document.createElement('div');
          missing.textContent = '[missing component ' + name + ']';
          target.appendChild(missing);
        }
        continue;
      }
      // localName: the uppercase tagName would make an unknown (inline) element.
      const out = document.createElementNS(child.namespaceURI, child.localName);
      for (const attr of Array.from(child.attributes)) {
        if (isHandler(attr.name) || attr.name.startsWith('hint-')) continue;
        const value = interpolate(scopes, attr.value);
        // A boolean attribute whose expression was false is left out.
        if (/^(disabled|checked|selected|hidden|readonly|open)$/i.test(attr.name) && (value === '' || value === 'false')) continue;
        out.setAttribute(attr.name, value);
      }
      expand(child, scopes, out, components);
      target.appendChild(out);
    }
  }

  // Renders one component (its template and logic) into `host`.
  function render(component, props, host, components) {
    const Logic = component.logic;
    const instance = new Logic(props);
    const vals = instance.renderVals() || {};
    const scopes = [props, vals];
    const template = document.createElement('template');
    template.innerHTML = component.template;
    const root = template.content;
    for (const helmet of Array.from(root.querySelectorAll('helmet'))) {
      for (const style of Array.from(helmet.querySelectorAll('style'))) {
        const hoisted = document.createElement('style');
        hoisted.textContent = interpolate(scopes, style.textContent || '');
        document.head.appendChild(hoisted);
      }
      helmet.remove();
    }
    expand(root, scopes, host, components);
  }

  // Entry: `window.DC_RENDER = { root: 'R2App', props: {...}, components: {R2App: {template, logic}, …} }`.
  window.dcRender = function (spec) {
    const host = document.getElementById('dc-root');
    render(spec.components[spec.root], spec.props, host, spec.components);
    document.documentElement.setAttribute('data-dc-rendered', '1');
  };
})();
