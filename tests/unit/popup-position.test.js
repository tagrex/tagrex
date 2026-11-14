import {test} from 'node:test';
import assert from 'node:assert/strict';
import {placeFloating,placeContextMenu} from '../../app/ui/js/popup-position.js';
function fixture({width=160,height=200,x=0,y=0,scale=1}={}){
  globalThis.innerWidth=800;globalThis.innerHeight=600;
  const style={};return {style,isConnected:true,hidden:false,
    get offsetWidth(){return parseFloat(style.width)||width;},
    get offsetHeight(){return Math.min(height,parseFloat(style.maxHeight)||height);},
    getBoundingClientRect(){const left=x+(parseFloat(style.left)||0)*scale,top=y+(parseFloat(style.top)||0)*scale;const w=this.offsetWidth*scale,h=this.offsetHeight*scale;return {x:left,y:top,left,top,right:left+w,bottom:top+h,width:w,height:h};}};
}
const anchor=(left,top,width=100,height=32)=>({getBoundingClientRect:()=>({left,top,right:left+width,bottom:top+height,width,height})});
test('field popup attaches below its whole control',()=>{const box=fixture();placeFloating(box,anchor(100,80),{width:100});assert.equal(box.getBoundingClientRect().left,100);assert.equal(box.getBoundingClientRect().top,116);assert.equal(box.offsetWidth,100);});
test('trailing action aligns its right edge and flips above when needed',()=>{const box=fixture();placeFloating(box,anchor(600,540),{align:'right'});const r=box.getBoundingClientRect();assert.equal(r.right,700);assert.equal(r.bottom,536);});
test('translated and scaled dialogs retain viewport anchor coordinates',()=>{const box=fixture({x:250,y:120,scale:.8});placeFloating(box,anchor(420,280));assert.equal(box.getBoundingClientRect().left,420);assert.equal(box.getBoundingClientRect().top,316);});
test('context menus start at the pointer and clamp near window edges',()=>{const box=fixture();placeContextMenu(box,500,300);assert.equal(box.getBoundingClientRect().left,500);assert.equal(box.getBoundingClientRect().top,300);placeContextMenu(box,799,599);assert.equal(box.getBoundingClientRect().right,792);assert.equal(box.getBoundingClientRect().bottom,592);});
test('oversized requested field width is bounded by viewport gutters',()=>{const box=fixture();placeFloating(box,anchor(0,10),{width:900});assert.equal(box.offsetWidth,784);assert.equal(box.getBoundingClientRect().left,8);});
test('short windows limit the popup height rather than losing endpoints',()=>{const box=fixture({height:1000});globalThis.innerHeight=180;placeFloating(box,anchor(100,40));assert.equal(box.offsetHeight,96);assert.equal(box.getBoundingClientRect().bottom,172);});

test('fractional natural widths are not mistaken for a dialog scale',()=>{const box=fixture({width:168.35});Object.defineProperty(box,'offsetWidth',{get:()=>168});const read=box.getBoundingClientRect.bind(box);box.getBoundingClientRect=()=>{const r=read();return {...r,width:168.35,right:r.left+168.35};};globalThis.getComputedStyle=()=>({width:'168.35px',height:'200px',boxSizing:'border-box'});try{placeFloating(box,anchor(500,80));assert.equal(box.getBoundingClientRect().left,500);}finally{delete globalThis.getComputedStyle;}});
