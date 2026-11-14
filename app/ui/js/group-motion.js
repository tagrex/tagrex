// Virtual membership changes once. Only viewport rows animate, using compositor
// transforms/opacity; no prefix-array rebuild or layout writes on animation frames.
export function createGroupMotion(wrap, body) {
  const reduced = matchMedia('(prefers-reduced-motion: reduce)');
  const animations = new Set();
  let ghost = null;
  let lastRun = -Infinity;
  const caretOf = group => [...body.querySelectorAll('tr.group-head')].find(row => row.dataset.group === group)?.querySelector('.group-caret > svg');
  function cancel() {
    for (const animation of animations) animation.cancel();
    animations.clear(); ghost?.remove(); ghost = null;
  }
  function animate(node, frames, duration = 120) {
    const animation = node.animate(frames, { duration, easing:'cubic-bezier(.2,.7,.2,1)' });
    animations.add(animation);
    animation.finished.catch(() => {}).finally(() => animations.delete(animation));
    return animation;
  }
  const key = row => row.dataset.path ? `file:${row.dataset.path}` : `group:${row.dataset.group}`;
  function run(change, { group = null, collapsing = false } = {}) {
    const now = performance.now();
    const rapid = now - lastRun < 160;
    lastRun = now;
    const oldCaret = rapid && !reduced.matches ? caretOf(group) : null;
    const rotation = oldCaret ? getComputedStyle(oldCaret).transform : null;
    cancel();
    if (reduced.matches || typeof Element.prototype.animate !== 'function') { change(); return; }
    // During a burst, apply every requested state immediately. Recreating row
    // snapshots/players only makes the interface trail the pointer. Continue
    // just the caret from its current angle, with no animation queue.
    if (rapid) {
      change();
      const caret = caretOf(group);
      if (caret && rotation) animate(caret,[{transform:rotation},{transform:collapsing?'rotate(-90deg)':'rotate(0)'}],80);
      return;
    }
    const viewport = wrap.getBoundingClientRect();
    const top = Math.max(viewport.top, wrap.querySelector('thead').getBoundingClientRect().bottom);
    const before = new Map();
    const widths = [...(body.querySelector('tr[data-path]')?.cells || [])].map(cell => cell.getBoundingClientRect().width);
    const cellStyles = new Map();
    const exiting = [];
    // Offscreen overscan is ignored; large groups never create a large snapshot.
    for (const row of body.querySelectorAll('tr[data-path],tr.group-head')) {
      const rect = row.getBoundingClientRect();
      if (rect.bottom <= top || rect.top >= viewport.bottom || before.size >= 80) continue;
      before.set(key(row), rect.top);
      if (!collapsing || !row.dataset.path || row.dataset.group !== group) continue;
      const clone = row.cloneNode(true);
      clone.removeAttribute('data-path'); clone.removeAttribute('data-group');
      clone.removeAttribute('tabindex'); clone.removeAttribute('aria-rowindex');
      Object.assign(clone.style, {position:'absolute',display:'table',tableLayout:'fixed',left:`${rect.left-viewport.left}px`,top:`${rect.top-top}px`,width:`${rect.width}px`,height:`${rect.height}px`,margin:'0'});
      [...row.cells].forEach((cell,index) => {
        const cacheKey = `${row.className}|${Number(row.getAttribute('aria-rowindex')) % 2}|${cell.className}`;
        let style = cellStyles.get(cacheKey);
        if (!style) {
          const computed = getComputedStyle(cell);
          const declaration = document.createElement('td').style;
          Object.assign(declaration,{boxSizing:'border-box',display:computed.display,padding:computed.padding,color:computed.color,background:computed.backgroundColor,font:computed.font,lineHeight:computed.lineHeight,whiteSpace:computed.whiteSpace,overflow:'hidden',textOverflow:computed.textOverflow,border:'0'});
          style = declaration.cssText;
          cellStyles.set(cacheKey,style);
        }
        const copy = clone.cells[index];
        copy.style.cssText = `${style};width:${widths[index]}px`;
        copy.querySelectorAll('[id],[tabindex],[contenteditable]').forEach(node => {node.removeAttribute('id');node.removeAttribute('tabindex');node.removeAttribute('contenteditable');});
      });
      exiting.push(clone);
    }
    change();
    // Batch geometry reads before creating animations. Entering rows have a
    // smaller animation budget so expansion doesn't start dozens of players.
    let animatedRows = 0, enteringRows = 0;
    const visible = [];
    for (const row of body.querySelectorAll('tr[data-path],tr.group-head')) {
      const rect = row.getBoundingClientRect();
      if (rect.bottom <= top || rect.top >= viewport.bottom) continue;
      if (++animatedRows > 80) break;
      visible.push({row,top:rect.top});
    }
    for (const {row,top:rowTop} of visible) {
      const previous = before.get(key(row));
      if (previous !== undefined && Math.abs(previous-rowTop) > .5) {
        animate(row,[{transform:`translateY(${previous-rowTop}px)`},{transform:'translateY(0)'}]);
      } else if (previous === undefined && row.dataset.path && enteringRows++ < 8) {
        animate(row,[{opacity:0,transform:'translateY(-4px)'},{opacity:1,transform:'translateY(0)'}]);
      }
      if (row.classList.contains('group-head') && row.dataset.group === group) {
        const caret = row.querySelector('.group-caret > svg');
        if (caret) animate(caret,[{transform:collapsing?'rotate(0)':'rotate(-90deg)'},{transform:collapsing?'rotate(-90deg)':'rotate(0)'}]);
      }
    }
    if (exiting.length) {
      ghost = document.createElement('div');
      ghost.className = 'group-motion-exit'; ghost.classList.toggle('show-old',wrap.querySelector('#tracks').classList.contains('show-old')); ghost.setAttribute('aria-hidden','true'); ghost.inert = true;
      Object.assign(ghost.style,{position:'fixed',pointerEvents:'none',overflow:'hidden',contain:'layout paint',left:`${viewport.left}px`,top:`${top}px`,width:`${wrap.clientWidth}px`,height:`${Math.max(0,viewport.bottom-top)}px`,zIndex:'25'});
      const table = document.createElement('table'), rows = document.createElement('tbody');
      table.style.borderSpacing = '0'; rows.append(...exiting); table.append(rows); ghost.append(table); document.body.append(ghost);
      const current = ghost;
      animate(ghost,[{opacity:1,transform:'translateY(0)'},{opacity:0,transform:'translateY(-5px)'}],90).finished.catch(() => {}).finally(() => {current.remove();if(ghost===current)ghost=null;});
    }
  }
  wrap.addEventListener('scroll',cancel,{passive:true});
  function pointerDown(event) {
    // A header reversal samples the running caret before cancelling it.
    if (!event.target.closest('tr.group-head') || event.ctrlKey || event.metaKey || event.button !== 0) cancel();
  }
  wrap.addEventListener('pointerdown',pointerDown,{capture:true,passive:true});
  window.addEventListener('resize',cancel,{passive:true});
  window.addEventListener('pagehide',() => {cancel();wrap.removeEventListener('scroll',cancel);wrap.removeEventListener('pointerdown',pointerDown,true);window.removeEventListener('resize',cancel);},{once:true});
  return { run, cancel };
}
