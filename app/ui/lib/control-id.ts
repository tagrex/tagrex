let serial = 0;
export function nextControlId(prefix = 'control') { return `${prefix}-${++serial}`; }
