/**
 * Capability-backed format selection. No format IDs or extensions are baked in.
 * This module is deliberately runtime-independent and tested with node:test.
 */
export function findFormatByExtension(capabilities, extension) {
  const ext = extension.replace(/^\./, "").toLowerCase();
  for (const family of capabilities?.families ?? []) {
    const descriptor = family.descriptors.find(d =>
      d.can_read && [d.canonical_extension, ...d.aliases]
        .some(alias => alias.toLowerCase() === ext)
    );
    if (descriptor) return { family: family.family, descriptor };
  }
  return null;
}

export function availableOutputFormats(capabilities, familyName, inputExtensions = []) {
  const family = capabilities?.families.find(f => f.family === familyName);
  if (!family) return [];

  const writable = new Set(family.descriptors.filter(d => d.can_write).map(d => d.id));
  const routes = inputExtensions.length
    ? inputExtensions.map(ext => {
        const matching = family.descriptors.find(d =>
          d.can_read && [d.canonical_extension, ...d.aliases]
            .some(alias => alias.toLowerCase() === ext.replace(/^\./, "").toLowerCase())
        );
        if (!matching) return [];
        return family.routes.find(r => r.input === matching.id)?.outputs ?? [];
      })
    : family.routes.map(r => r.outputs);

  if (!routes.length) return [];
  const allowed = inputExtensions.length
    ? routes.reduce((intersection, options) => new Set(options.filter(id => intersection.has(id))), new Set(routes[0]))
    : new Set(routes.flat());

  return family.descriptors
    .filter(d => d.can_write && writable.has(d.id) && allowed.has(d.id))
    .map(d => d.id);
}

export function pickSupportedFormat(preferred, available) {
  return available.includes(preferred) ? preferred : (available[0] ?? "");
}

export function filterOutputFormats(formats, query) {
  const term = query.trim().toLowerCase();
  return term ? formats.filter(f => f.toLowerCase().includes(term)) : formats;
}
