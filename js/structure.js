const allowed = new Set(['parents', 'labels', 'requireChildToParent', 'requireParentToChild', 'requireBreadcrumbs', 'breadcrumbSelector', 'rootLabel']);
const structures = new WeakSet();
function record(value, name) {
  if (!value || typeof value !== 'object' || Array.isArray(value) || ![Object.prototype, null].includes(Object.getPrototypeOf(value))) throw new TypeError(`${name} must be a plain data object`);
  for (const key of Reflect.ownKeys(value)) {
    if (typeof key !== 'string' || !('value' in Object.getOwnPropertyDescriptor(value, key))) throw new TypeError(`${name} must contain data properties only`);
  }
}
function path(value) {
  if (typeof value !== 'string' || !value.startsWith('/') || value.startsWith('//') || /[?#\\\s]/u.test(value) || value.split('/').some(p => p === '.' || p === '..')) throw new TypeError(`Invalid hierarchy path: ${String(value)}`);
}
export function defineStructure(input) {
  record(input, 'structure');
  for (const key of Object.keys(input)) if (!allowed.has(key)) throw new TypeError(`Unknown hierarchy option: ${key}`);
  record(input.parents, 'parents');
  record(input.labels, 'labels');
  const parents = Object.create(null), labels = Object.create(null);
  for (const [key, label] of Object.entries(input.labels)) {
    path(key);
    if (typeof label !== 'string' || !label.trim()) throw new TypeError(`Explicit nonempty label required: ${key}`);
    labels[key] = label;
  }
  for (const [child, parent] of Object.entries(input.parents)) {
    path(child); path(parent);
    if (!Object.hasOwn(labels, child) || !Object.hasOwn(labels, parent)) throw new TypeError(`Explicit labels required for ${child} and ${parent}`);
    parents[child] = parent;
  }
  for (const child of Object.keys(parents)) {
    const seen = new Set();
    let current = child;
    while (Object.hasOwn(parents, current)) {
      if (seen.has(current)) throw new TypeError(`Hierarchy cycle: ${child}`);
      seen.add(current); current = parents[current];
    }
  }
  const output = { parents: Object.freeze(parents), labels: Object.freeze(labels) };
  for (const key of ['requireChildToParent', 'requireParentToChild', 'requireBreadcrumbs']) {
    if (input[key] !== undefined && typeof input[key] !== 'boolean') throw new TypeError(`${key} must be boolean`);
    if (input[key] !== undefined) output[key] = input[key];
  }
  for (const key of ['breadcrumbSelector', 'rootLabel']) {
    if (input[key] !== undefined && (typeof input[key] !== 'string' || !input[key].trim())) throw new TypeError(`${key} must be a nonempty string`);
    if (input[key] !== undefined) output[key] = input[key];
  }
  if (output.requireBreadcrumbs && !output.breadcrumbSelector) throw new TypeError('requireBreadcrumbs requires breadcrumbSelector');
  Object.freeze(output); structures.add(output); return output;
}
function checked(structure, page) {
  if (!structures.has(structure)) throw new TypeError('Use defineStructure before querying hierarchy');
  path(page);
}
const item = (structure, path) => Object.freeze({ path, label: structure.labels[path] });
export function parentOf(structure, page) {
  checked(structure, page);
  return Object.hasOwn(structure.parents, page) ? item(structure, structure.parents[page]) : undefined;
}
export function childrenOf(structure, page) {
  checked(structure, page);
  return Object.keys(structure.parents).filter(child => structure.parents[child] === page).sort().map(child => item(structure, child));
}
export function breadcrumbsOf(structure, page) {
  checked(structure, page);
  const result = [];
  while (Object.hasOwn(structure.parents, page)) {
    page = structure.parents[page]; result.unshift(item(structure, page));
  }
  return result;
}
