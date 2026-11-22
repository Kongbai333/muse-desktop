// Focused offline regression: exact original bytes, no account and no generation.
const assert=require('node:assert/strict'),vm=require('node:vm'),fs=require('node:fs');
const source=fs.readFileSync(require.resolve('../assets/muse-studio.js'),'utf8');

function fixture({viewer=true,large=false}={}) {
  let time=0,clicks=0,viewerClicks=0;
  const timers=[],fetches=[],messages=[],boxes=[],images=[];
  class FakeDate extends Date {static now(){return time;}}
  class Area {
    constructor(){this._value='';this.disabled=false;}
    get value(){return this._value;}set value(value){this._value=value;}
    dispatchEvent(){}getClientRects(){return[{}];}closest(){return null;}
  }
  const area=new Area();
  const button={dataset:{testid:'hatch-composer-send-button'},disabled:false,getClientRects:()=>[{}],closest:()=>null,getAttribute:()=>null,
    click(){clicks++;area.value='';messages.push({className:'chat-user-bubble',innerText:'fixture prompt'});}};
  area.parentElement={querySelectorAll:()=>[button],matches:()=>true,parentElement:null};
  const viewerRoot={querySelector:()=>null,dispatchEvent(event){if(event.key==='Escape')full.shown=false;}};
  function image(url,width,height,shown=true){return {
    currentSrc:url,src:url,naturalWidth:width,naturalHeight:height,complete:true,shown,
    getClientRects(){return this.shown?[{}]:[];},
    getAttribute(name){return name==='src'?this.currentSrc:null;},
    closest(selector){if(selector.includes('hatch-chat-attachment'))return this===thumb?box:null;
      if(selector.includes('button'))return this===thumb?openButton:null;
      if(selector.includes('dialog')||selector.includes('aria-modal'))return this===full && this.shown?viewerRoot:null;
      return null;},
    click(){if(this===thumb)openButton.click();},
  };}
  const thumb=image('blob:https://muse.ai/selected-thumb',large?1280:498,large?720:280);
  const full=image('blob:https://muse.ai/old-viewer',1792,1008,false);
  const unrelated=image('blob:https://muse.ai/unrelated',4096,2304,true);
  const openButton={disabled:false,getAttribute:()=>null,click(){viewerClicks++;
    // The stale full-size viewer is shown first; it is not the selected original.
    if(viewer)full.shown=true;
    if(viewer)timers.push({at:time+2200,callback(){full.shown=true;full.src=full.currentSrc='blob:https://muse.ai/selected-original';full.naturalWidth=1792;full.naturalHeight=1008;}});
  }};
  const box={closest:()=>null,contains:element=>element===thumb||element===openButton,
    getAttribute:()=> 'hatch-chat-attachment-presentation-fixture',
    querySelector:selector=>selector==='img'?thumb:null,querySelectorAll:()=>[thumb]};
  images.push(full,unrelated);
  const bytes={
    'blob:https://muse.ai/selected-thumb':new Uint8Array([1,2,3]),
    'blob:https://muse.ai/selected-original':new Uint8Array([9,8,7,6,5,4]),
    'blob:https://muse.ai/unrelated':new Uint8Array([40,41]),
  };
  const document={body:{},activeElement:viewerRoot,querySelector:()=>null,dispatchEvent:event=>viewerRoot.dispatchEvent(event),
    querySelectorAll(selector){
      if(selector==='textarea')return[area];
      if(selector.includes('hatch-chat-groupable-bubble'))return messages;
      if(selector.includes('hatch-chat-attachment-presentation-')&&!selector.includes(' img'))return boxes;
      if(selector==='img')return images;
      if(selector.includes('dialog')||selector.includes('radix'))return full.shown?[full]:[];
      return[];
    }};
  const context={document,location:new URL('https://muse.ai/thread/fixture'),HTMLTextAreaElement:Area,Event:class{},
    KeyboardEvent:class{constructor(type,values){Object.assign(this,values);this.type=type;}},
    AbortController,URL,Uint8Array,Date:FakeDate,Node:{DOCUMENT_POSITION_FOLLOWING:4},
    setTimeout(callback,delay){timers.push({at:time+delay,callback});return timers.length;},
    getComputedStyle:element=>({visibility:element.shown===false?'hidden':'visible'}),
    btoa:value=>Buffer.from(value,'binary').toString('base64'),atob:value=>Buffer.from(value,'base64').toString('binary'),
    fetch:async url=>{
      fetches.push(String(url));const data=bytes[String(url)]||new Uint8Array([0]);
      return{ok:true,headers:new Headers({'content-type':'image/png','content-length':String(data.length)}),
        body:new ReadableStream({start(controller){controller.enqueue(data);controller.close();}}),
        arrayBuffer:async()=>data.buffer};
    }};
  context.window=context;context.top=context;vm.runInNewContext(source,context);
  return {api:context.__museStudio,fetches,
    attach(){boxes.push(box);images.push(thumb);messages.push({className:'hatch-agent-bubble-bg',innerText:'generated'});},
    async advance(){time+=250;for(let i=timers.length-1;i>=0;i--)if(timers[i].at<=time){const task=timers.splice(i,1)[0];task.callback();}await new Promise(resolve=>setImmediate(resolve));},
    get clicks(){return clicks;},get viewerClicks(){return viewerClicks;},bytes,
  };
}

async function finish(page,id,steps=140){let reply;for(let i=0;i<steps;i++){
  await page.advance();reply=JSON.parse(page.api.poll(id));if(['media','error'].includes(reply.state))return reply;
}throw Error('fixture did not finish');}

(async()=>{
  const delayed=fixture();
  delayed.api.begin('original','image','fixture prompt');delayed.api.poll('original');delayed.attach();
  const reply=await finish(delayed,'original');
  assert.equal(reply.state,'media');
  assert.equal(delayed.fetches.at(-1),'blob:https://muse.ai/selected-original','a reused viewer that loads after 2.2s must supply the original, never the 498px chat thumbnail');
  assert.equal(reply.original,true);assert.equal(reply.width,1792);assert.equal(reply.height,1008);
  const chunk=JSON.parse(delayed.api.read('original',0));
  assert.deepEqual([...Buffer.from(chunk.data,'base64')],[9,8,7,6,5,4],'downloaded bytes must be unchanged');
  assert.equal(delayed.fetches.includes('blob:https://muse.ai/unrelated'),false);

  const large=fixture({large:true});large.api.begin('large','image','fixture prompt');large.api.poll('large');large.attach();
  const largeReply=await finish(large,'large');assert.equal(largeReply.original,true);
  assert.equal(large.fetches.at(-1),'blob:https://muse.ai/selected-original','a 1280px chat preview still needs the actual original');

  const missing=fixture({viewer:false});missing.api.begin('missing','image','fixture prompt');missing.api.poll('missing');missing.attach();
  const failure=await finish(missing,'missing');assert.equal(failure.state,'error','an unavailable original cannot silently complete with a thumbnail');assert.equal(missing.fetches.length,0);

  const recovery=fixture();recovery.attach();
  assert.equal(typeof recovery.api.recover,'function');
  recovery.api.recover('recovery',{thumbnail:Buffer.from([1,2,3]).toString('base64')});
  const restored=await finish(recovery,'recovery');assert.equal(restored.original,true);assert.equal(recovery.clicks,0,'recovery must not send a message or generate again');
  console.log('Original media contract passed: delayed reused viewer, exact bytes, unrelated-image rejection, no thumbnail fallback and read-only recovery.');
})().catch(error=>{console.error(error);process.exitCode=1;});
