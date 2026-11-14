import test from 'node:test';
import assert from 'node:assert/strict';
import { setImmediate as flush } from 'node:timers/promises';

let sequence=0;
async function setup(saved=null, blocked=false, load=()=>Promise.resolve([])) {
  const originals=Object.fromEntries(['document','localStorage'].map(key=>[key,Object.getOwnPropertyDescriptor(globalThis,key)]));
  const entries=new Map(saved===null?[]:[['tagrex.interfaceFont',saved]]);
  entries.set('tagrex.valueFont','condensed');
  const root={dataset:{}};
  const events=[];
  globalThis.document={documentElement:root,dispatchEvent:event=>events.push(event.type),fonts:{load}};
  globalThis.localStorage={getItem:key=>{if(blocked)throw Error('blocked');return entries.get(key)??null;},setItem:(key,value)=>{if(blocked)throw Error('blocked');entries.set(key,value);}};
  const module=await import(`../../app/ui/js/interface-fonts.js?test=${sequence++}`);
  return {module,root,entries,events,restore(){for(const [key,descriptor] of Object.entries(originals)){if(descriptor)Object.defineProperty(globalThis,key,descriptor);else delete globalThis[key];}}};
}

test('four interface choices initialize before paint and retain separate data-font preference',async()=>{
  const h=await setup('source');
  try {
    assert.deepEqual(h.module.INTERFACE_FONTS,['inter','plex','source','mono']);
    assert.equal(h.root.dataset.uiFont,'source');
    for(const face of h.module.INTERFACE_FONTS) {
      assert.equal(h.module.applyInterfaceFont(face),face);
      assert.equal(h.root.dataset.uiFont,face);
      assert.equal(h.entries.get('tagrex.interfaceFont'),face);
      assert.equal(h.entries.get('tagrex.valueFont'),'condensed');
    }
    await flush();
  } finally { h.restore(); }
});

test('invalid or blocked preferences fall back to Inter without preventing display',async()=>{
  for(const blocked of [false,true]) {
    const h=await setup('obsolete',blocked);
    try {
      assert.equal(h.root.dataset.uiFont,'inter');
      h.module.applyInterfaceFont('mono');
      assert.equal(h.root.dataset.uiFont,'mono');
      assert.equal(h.module.applyInterfaceFont('unknown'),'inter');
      await flush();
    } finally {h.restore();}
  }
});

test('a late font load cannot refresh a superseded family; failed font loads stay usable',async()=>{
  const pending=[];
  const h=await setup(null,false,()=>new Promise((resolve,reject)=>pending.push({resolve,reject})));
  try {
    h.module.applyInterfaceFont('plex');
    h.module.applyInterfaceFont('source');
    assert.equal(h.events.length,2);
    pending[0].resolve([]);await flush();
    assert.equal(h.events.length,2);
    pending[1].reject(Error('missing asset'));await flush();
    assert.equal(h.events.length,3);
    assert.equal(h.root.dataset.uiFont,'source');
  } finally {h.restore();}
});
