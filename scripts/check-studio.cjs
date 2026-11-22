// One focused contract check, without running the wider regression suites.
const assert=require('node:assert/strict'),vm=require('node:vm'),fs=require('node:fs');
const source=fs.readFileSync(require.resolve('../assets/muse-studio.js'),'utf8');
function page(origin){
  let clicks=0,fetches=0;
  class Area {constructor(){this._value='';this.disabled=false;}get value(){return this._value;}set value(v){this._value=v;}dispatchEvent(){}getClientRects(){return [{}];}closest(){return null;}}
  const ta=new Area(),messages=[{className:'hatch-chat-groupable-bubble hatch-agent-bubble-bg',innerText:'old reply'}],atts=[];
  const button={dataset:{testid:'hatch-composer-send-button'},disabled:false,getClientRects:()=>[{}],closest:()=>null,getAttribute:()=>null,click(){clicks++;ta.value='';messages.push({className:'hatch-chat-groupable-bubble chat-user-bubble',innerText:'sent prompt'});}};
  const composer={parentElement:null,querySelectorAll:()=>[button],matches:()=>true};ta.parentElement=composer;
  const document={body:{},querySelector:()=>null,querySelectorAll(selector){
    if(selector==='textarea')return [ta];
    if(selector.includes('hatch-chat-groupable-bubble'))return messages;
    if(selector.includes('hatch-chat-attachment-presentation-'))return atts;
    return [];
  }};
  const context={document,location:new URL(origin),HTMLTextAreaElement:Area,Event:class{},AbortController,URL,Uint8Array,setTimeout,Date,
    getComputedStyle:()=>({visibility:'visible'}),btoa:s=>Buffer.from(s,'binary').toString('base64'),
    fetch:async()=>{fetches++;return {ok:true,headers:new Headers({'content-type':'image/png'}),body:new ReadableStream({start(c){c.enqueue(new Uint8Array([1,2,3]));c.close();}})};}};
  context.window=context;context.top=context;
  vm.runInNewContext(source,context);
  return {api:context.__museStudio,messages,atts,get clicks(){return clicks;},get fetches(){return fetches;}};
}
(async()=>{
  const denied=page('https://example.test/');
  assert.equal(JSON.parse(denied.api.begin('a','chat','hello')).state,'error');assert.equal(denied.clicks,0);
  const p=page('https://muse.ai/thread/new');
  assert.equal(JSON.parse(p.api.begin('a','chat','hello')).state,'preparing');
  assert.equal(JSON.parse(p.api.poll('a')).state,'sending');assert.equal(p.clicks,1);
  assert.equal(JSON.parse(p.api.poll('a')).text,'','old assistant text must not become the new answer');
  p.messages.push({className:'hatch-chat-groupable-bubble hatch-agent-bubble-bg',innerText:'new reply'});
  let reply;for(let i=0;i<8;i++)reply=JSON.parse(p.api.poll('a'));
  assert.equal(reply.state,'done');assert.equal(reply.text,'new reply');
  assert.equal(JSON.parse(p.api.poll('stale')).state,'error');
  p.api.release('a');p.api.begin('b','video','make video');p.api.poll('b');
  const video={readyState:2,currentSrc:'https://muse.ai/result.mp4'};
  p.atts.push({closest:()=>null,querySelector:selector=>selector==='video'?video:null,querySelectorAll:()=>[]});
  p.api.poll('b');await new Promise(resolve=>setImmediate(resolve));
  reply=JSON.parse(p.api.poll('b'));
  assert.equal(p.fetches,1);assert.equal(reply.state,'error','an image response cannot complete a video job');
  console.log('Studio adapter contract passed: origin, send receipt, new replies, stale jobs and media type.');
})().catch(error=>{console.error(error);process.exitCode=1;});
