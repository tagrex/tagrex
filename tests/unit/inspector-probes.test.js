import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';
import { setImmediate as flush } from 'node:timers/promises';

function functions(file, names) {
  const source=readFileSync(new URL(`../../app/ui/js/${file}`,import.meta.url),'utf8');
  const ast=ts.createSourceFile(file,source,ts.ScriptTarget.Latest,true,ts.ScriptKind.JS);
  return ast.statements.filter(n=>ts.isFunctionDeclaration(n)&&names.includes(n.name?.text)).map(n=>n.getText(ast)).join('\n');
}
const nameSource=functions('fromname.js',['refreshNameProbe','scheduleNameProbe']);
const textSource=functions('fromtext.js',['sampleTextWindow','refreshTextProbe','scheduleTextProbe','renderCount']);
function setup(kind, invoke) {
  let revision=1;
  const nodes={
    'from-name-probe':{innerHTML:'unchanged'},'from-text-probe':{innerHTML:'unchanged'},
    'from-name-mask':{value:'mask'},'from-text-mask':{value:'mask'},
    'from-text-lines':{value:'first\nsecond'},'from-text-count':{textContent:''},
    'subtab-fromtext':{hidden:false},'subtab-filename':{hidden:false},
  };
  const calls=[];
  const context=vm.createContext({
    el:id=>nodes[id],invoke,
    captureSelection:()=>({revision,paths:['/1.mp3','/2.mp3','/3.mp3','/4.mp3']}),
    isSelectionCurrent:subject=>subject.revision===revision,
    throughChain:async (_,fields)=>fields,
    renderProbeTracks:(box,samples)=>{calls.push(samples);box.innerHTML='rendered';},
    probeMessage:(box,message)=>{box.innerHTML=message;},
    t:key=>key,tn:(key,n)=>`${key}:${n}`,escapeHtml:x=>x,visibleSubject:x=>x,
    setTimeout:()=>1,clearTimeout:()=>{},
  });
  vm.runInContext(`let nameProbeGeneration=0,textProbeGeneration=0,nameProbeTimer=null,textProbeTimer=null;\n${kind==='name'?nameSource:textSource}`,context);
  return {context,nodes,calls,changeSelection:()=>revision++};
}
const probe=subject=>({subject,matched:true,fields:[['title',subject]],invalid:[]});

test('name probes ignore a late selection response and sample at most three paths', async()=>{
  let resolve;const requests=[];
  const h=setup('name',(_,args)=>{requests.push(args);return new Promise(r=>resolve=r);});
  const pending=h.context.refreshNameProbe();
  h.changeSelection(); resolve({files:[probe('old')]}); await pending;
  assert.equal(h.nodes['from-name-probe'].innerHTML,'unchanged');
  assert.equal(requests[0].paths.length,3);
});

test('a pattern change and a scheduled refresh invalidate older name results, even on A→B→A', async()=>{
  let resolve;const h=setup('name',()=>new Promise(r=>resolve=r));
  const pending=h.context.refreshNameProbe();
  h.context.scheduleNameProbe();
  resolve({files:[probe('old')]});await pending;
  assert.equal(h.calls.length,0);
});

test('text sample results cannot overwrite a newer edit or selection', async()=>{
  let resolve;
  const h=setup('text',name=>name==='probe_tags_from_text'?Promise.resolve({first:probe('first'),tracks:2,skipped:0}):new Promise(r=>resolve=r));
  const pending=h.context.refreshTextProbe(); await flush();
  h.nodes['from-text-lines'].value='newer text';h.changeSelection();
  resolve({lines:[{...probe('first'),line:'first',path:'/1.mp3'}]});await pending;
  assert.equal(h.calls.length,0);
  assert.equal(h.nodes['from-text-count'].textContent,'');
});

test('bounded text sample preserves skipped lines and file order without returning every pasted line', async()=>{
  const requests=[];
  const h=setup('text',async(name,args)=>{
    requests.push({name,args});
    if(name==='probe_tags_from_text')return {first:probe('first'),tracks:100000,skipped:1};
    return {lines:[{...probe('first'),line:'first',path:'/1.mp3'},{matched:false,line:'skip',fields:[]},{...probe('second'),line:'second',path:'/2.mp3'},{...probe('third'),line:'third',path:'/3.mp3'}]};
  });
  h.nodes['from-text-lines'].value='header\r\nfirst\r\nskip\r\nsecond\r\nthird\r\n'+'other\n'.repeat(100000);
  await h.context.refreshTextProbe();
  const sample=requests.find(r=>r.name==='probe_text_lines').args;
  assert.ok(sample.text.length<1000);
  assert.equal(sample.paths.length,3);
  assert.deepEqual(JSON.parse(JSON.stringify(h.calls[0].map(s=>[s.path,s.fields[0][1]]))),[['/1.mp3','first'],['/2.mp3','second'],['/3.mp3','third']]);
});

test('a sample starts on a whole line and falls back safely when its subject is absent',()=>{
  const h=setup('text',async()=>{});
  assert.equal(h.context.sampleTextWindow('prefix first suffix\n  first  \nsecond','first'),'  first  \nsecond');
  assert.equal(h.context.sampleTextWindow('prefix first suffix\nsecond','first'),null);
});

const onlineSource=functions('online.js',['runSearch','updateLoadMoreUi','searchBusy']);
function searchSetup(invoke) {
  const nodes=Object.fromEntries(['online-source','discogs-query','search-per-page','search-format','discogs-empty','online-empty-heading','discogs-search','release-more','load-more'].map(id=>[id,{value:'',hidden:false,dataset:{},classList:{toggle(){}},setAttribute(){}}]));
  nodes['online-source'].value='musicbrainz';
  nodes['search-per-page'].value='5';
  const notices=[];
  const cache=()=>new Map();
  const context=vm.createContext({
    el:id=>nodes[id],invoke,providerToken:async()=>'',t:key=>key,toast:message=>notices.push(message),
    prefetchReleaseCounts(){},renderReleaseList(){},
    releaseCache:cache(),coverCache:cache(),artworkChoices:cache(),artworkRotations:cache(),artworkImages:cache(),imageCache:cache(),expandedIds:cache(),
  });
  vm.runInContext(`let loadingResults=false,prefetching=false,searchFailed=false,searchPage=0,searchGen=0,releaseSource='',searchPerPage=5,searchHasMore=false,releaseCandidates=[];\n${onlineSource}`,context);
  return {nodes,context,notices};
}

test('online empty states move through searching, no results, error and recovery', async()=>{
  let resolve,reject;
  const h=searchSetup(()=>new Promise((yes,no)=>{resolve=yes;reject=no;}));
  h.context.updateLoadMoreUi();
  assert.equal(h.nodes['online-empty-heading'].textContent,'inspector.searchEmptyHeading');
  const first=h.context.runSearch(true);await flush();
  assert.equal(h.nodes['online-empty-heading'].textContent,'inspector.searchingHeading');
  resolve([]);await first;
  assert.equal(h.nodes['online-empty-heading'].textContent,'inspector.searchNoneHeading');
  const second=h.context.runSearch(true);await flush();reject('network failure');await second;
  assert.equal(h.nodes['online-empty-heading'].textContent,'inspector.searchErrorHeading');
  const third=h.context.runSearch(true);await flush();resolve([{id:'release'}]);await third;
  assert.equal(h.nodes['discogs-empty'].hidden,true);
});

test('a superseded search failure cannot replace the current empty state or show an old error', async()=>{
  let reject;
  const h=searchSetup(()=>new Promise((_,no)=>{reject=no;}));
  const old=h.context.runSearch(true);await flush();
  vm.runInContext('searchGen++; loadingResults=false;',h.context);
  h.context.updateLoadMoreUi();
  reject('stale network error');await old;
  assert.equal(h.nodes['online-empty-heading'].textContent,'inspector.searchEmptyHeading');
  assert.equal(h.notices.length,0);
});
