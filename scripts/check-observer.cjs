const { test } = require('node:test');
const assert = require('node:assert/strict');
const adapter = require('../assets/muse-observer.js');
const sample = (usage) => ({ subscriptionType: 'PAID', usage });
// Browser fixtures use a real EventTarget, matching the window interface that
// the production observer uses for pointer, keyboard and wheel idle tracking.
function withBrowserEvents(context) {
  const events = new EventTarget();
  for (const method of ['addEventListener', 'removeEventListener', 'dispatchEvent']) {
    context[method] = events[method].bind(events);
  }
  return context;
}

test('subscription percentages and resets preserve their real units', () => {
  const q = adapter.parseSubscription(sample({ state:'AVAILABLE', percentUsed:28, resetsAt:2000000000 }));
  assert.equal(q.state,'known');
  assert.equal(q.used_percent,28);
  assert.equal(q.resets_at,2000000000000);
  assert.equal(adapter.parseSubscription({subscription:sample({state:'AVAILABLE',percent_used:0})}).used_percent,0);
});
test('plan label follows explicit subscription metadata instead of defaulting to free',()=>{
  assert.equal(adapter.planLabel({subscriptionType:'FREE'}),'免费版');
  assert.equal(adapter.planLabel({subscriptionType:'PAID',tier:{name:'Pro'}}),'Pro');
  assert.equal(adapter.planLabel({}),'套餐');
});
test('avatar capture uses only explicit account media and produces a bounded frame',()=>{
  const image={naturalWidth:128,naturalHeight:128,getBoundingClientRect:()=>({width:48,height:48}),closest:()=>null};
  let calls=0;
  const root={getComputedStyle:()=>({display:'block',visibility:'visible',opacity:'1'}),document:{
    querySelectorAll:selector=>selector.includes('data-hatch-avatar-host')?[image]:[],
    createElement:()=>({width:0,height:0,getContext:()=>({drawImage:()=>calls++}),toDataURL:()=> 'data:image/png;base64,AAAA'})}};
  assert.equal(adapter.captureAvatar(root),'data:image/png;base64,AAAA');assert.equal(calls,1);
  image.closest=()=>({});assert.equal(adapter.captureAvatar(root),null);
});
test('unreadable or tainted avatar media falls back without fetching arbitrary images',()=>{
  const image={naturalWidth:128,naturalHeight:128,getBoundingClientRect:()=>({width:48,height:48}),closest:()=>null};
  const root={getComputedStyle:()=>({display:'block',visibility:'visible',opacity:'1'}),document:{querySelectorAll:()=>[image],createElement:()=>({getContext:()=>({drawImage:()=>{throw Error('tainted');}})})}};
  assert.equal(adapter.captureAvatar(root),null);
  root.document.querySelectorAll=()=>[];assert.equal(adapter.captureAvatar(root),null);
});
test('avatar is retried promptly when account media loads after the ready document',()=>{
  const vm=require('node:vm'),fs=require('node:fs');let loaded=false;
  const media={naturalWidth:128,naturalHeight:128,getBoundingClientRect:()=>({width:48,height:48}),closest:()=>null};
  const context={URL,Headers,AbortController,TextDecoder,Uint8Array,setTimeout,clearTimeout,
    location:new URL('https://muse.ai/'),
    document:{readyState:'complete',body:{innerText:'Ready app',querySelectorAll:()=>[]},querySelectorAll:selector=>loaded && selector.includes('data-hatch-avatar-host')?[media]:[],
      createElement:()=>({getContext:()=>({drawImage:()=>{}}),toDataURL:()=> 'data:image/png;base64,AAAA'})},
    fetch:async()=>{throw Error('unexpected request');},getComputedStyle:()=>({display:'block',visibility:'visible',opacity:'1'})};
  context.window=context;context.top=context;
  vm.runInNewContext(fs.readFileSync(require.resolve('../assets/muse-observer.js'),'utf8'),withBrowserEvents(context));
  assert.equal(context.__museDesktopObserver.snapshot().avatar_png,null);
  loaded=true;assert.equal(context.__museDesktopObserver.snapshot().avatar_png,'data:image/png;base64,AAAA');
});
test('extra invitation and topup pool is separate from the subscription percentage', () => {
  const value={...sample({state:'AVAILABLE',percentUsed:28}),topupBalance:600,topupTotal:1000,topupRowValueLabel:'剩余 600 积分'};
  const result=adapter.parseSubscription(value);
  assert.equal(result.used_percent,28);
  assert.deepEqual(result.extra,{remaining:600,total:1000,display_value:'剩余 600 积分'});
  const snake=adapter.parseSubscription({...sample({state:'UNLIMITED'}),topup_balance:0,topup_total:200});
  assert.equal(snake.extra.remaining,0);
  assert.equal(snake.extra.total,200);
  assert.equal(adapter.parseSubscription(sample({state:'AVAILABLE',percentUsed:28})).extra,undefined);
});
test('extra pool preserves a display-only value and rejects invalid or contradictory numbers', () => {
  const base=sample({state:'AVAILABLE',percentUsed:28});
  assert.deepEqual(adapter.parseSubscription({...base,topupRowValueLabel:'已使用 12%'}).extra,{remaining:null,total:null,display_value:'已使用 12%'});
  assert.deepEqual(adapter.parseSubscription({...base,topupBalance:400}).extra,{remaining:400,total:null,display_value:null});
  for(const fields of [
    {topupBalance:-1,topupTotal:100}, {topupBalance:101,topupTotal:100},
    {topupBalance:NaN}, {topupBalance:'50'}, {topupTotal:-1},
    {topupBalance:20,topup_balance:30}, {topupTotal:0,topupBalance:1},
  ]) assert.equal(adapter.parseSubscription({...base,...fields}).extra,undefined);
  assert.equal(adapter.parseSubscription({...base,topupBalance:0,topupTotal:0}).extra.remaining,0);
});
test('invalid, conflicting and unrelated usage cannot become a quota ring', () => {
  for (const usage of [
    {state:'AVAILABLE',percentUsed:-1}, {state:'AVAILABLE',percentUsed:101},
    {state:'AVAILABLE',percentUsed:'50'}, {state:'AVAILABLE',percentUsed:NaN},
    {state:'AVAILABLE',percentUsed:10,percent_used:30},
    {state:'AVAILABLE',percentUsed:30,quotaStatus:'DEPLETED'},
  ]) assert.equal(adapter.parseSubscription(sample(usage)),null);
  assert.equal(adapter.parseSubscription({usage:{percentUsed:30},messages:['private']}),null);
  assert.equal(adapter.parseSubscription({subscriptionType:'PAID',usage:{state:'AVAILABLE'}}),null);
});
test('exhausted and unlimited are distinct from unknown', () => {
  assert.equal(adapter.parseSubscription(sample({quotaStatus:'DEPLETED'})).used_percent,100);
  assert.deepEqual(adapter.parseSubscription(sample({state:'UNLIMITED'})),{state:'unlimited',resets_at:null,plan_label:'订阅版'});
  assert.equal(adapter.parseSubscription(null),null);
});
test('RSC subscription replies are parsed without forwarding identity or page data', () => {
  const payload='0:{"a":"$@1"}\n1:'+JSON.stringify({subscription:sample({state:'AVAILABLE',percentUsed:64}),email:'private@example.test',messages:['private']})+'\n';
  const result=adapter.parseResponse(payload);
  assert.equal(result.used_percent,64);
  assert.equal(JSON.stringify(result).includes('private'),false);
  assert.equal(adapter.parseResponse('x'.repeat(600001)),null);
  assert.equal(adapter.parseResponse('1:{"messages":["used 100%"]}'),null);
});
test('only an explicitly named read-only subscription action is discovered', () => {
  const id='a'.repeat(40);
  assert.equal(adapter.discoverAction(`const x=(0,e.createServerReference)("${id}",e.callServer,void 0,e.maps,"fetchSubscriptionAction");`),id);
  assert.equal(adapter.discoverAction(`createServerReference("${id}",x,"sendMessageAction")`),null);
  assert.equal(adapter.discoverAction('fetchSubscriptionAction'),null);
});
test('only the observed settings preload module is eligible for lazy loading', () => {
  assert.equal(adapter.discoverPreload('x.A(12345).then(({preloadHatchSettingsData:e})=>e)'),12345);
  assert.equal(adapter.discoverPreload('x.A(12345).then(({deleteAccount:e})=>e)'),null);
});
test('origin restrictions reject lookalikes, credentials, unusual ports and HTTP', () => {
  assert.equal(adapter.trusted('https://muse.ai/chat'),true);
  assert.equal(adapter.trusted('https://www.muse.ai/'),true);
  for(const url of ['http://muse.ai/','https://muse.ai.evil.test/','https://muse.ai:444/','https://agent@muse.ai/']) assert.equal(adapter.trusted(url),false);
});
test('empty/loading documents and missing stylesheets are not ready', () => {
  const context={location:{href:'https://muse.ai/'},document:{readyState:'complete',body:{innerText:'Ready content',querySelectorAll:()=>[]},querySelectorAll:()=>[]},getComputedStyle:()=>({display:'block',visibility:'visible',opacity:'1'})};
  assert.equal(adapter.contentReady(context),true);
  context.document.readyState='loading';
  assert.equal(adapter.contentReady(context),false);
  context.document.readyState='complete';
  context.document.querySelectorAll=()=>[{sheet:null,disabled:false}];
  assert.equal(adapter.contentReady(context),false);
  context.document.querySelectorAll=()=>[];
  context.document.body.innerText='';
  assert.equal(adapter.contentReady(context),false);
});
test('the browser observer returns a serializable snapshot while lookup is pending', () => {
  const vm=require('node:vm');
  const fs=require('node:fs');
  const context={URL,Headers,AbortController,TextDecoder,Uint8Array,setTimeout,clearTimeout,
    location:new URL('https://muse.ai/'),
    document:{readyState:'complete',body:{innerText:'Visible app',querySelectorAll:()=>[]},querySelectorAll:()=>[]},
    fetch:async()=>{throw new Error('unexpected request');},
    getComputedStyle:()=>({display:'block',visibility:'visible',opacity:'1'})};
  context.window=context;context.top=context;
  vm.runInNewContext(fs.readFileSync(require.resolve('../assets/muse-observer.js'),'utf8'),withBrowserEvents(context));
  const snapshot=JSON.parse(JSON.stringify(context.__museDesktopObserver.snapshot()));
  assert.equal(snapshot.ready,true);
  assert.equal(snapshot.quota,null);
  assert.equal(snapshot.url,'https://muse.ai/');
  assert.equal(snapshot.document_state,'complete');
});
test('a subscription reply from before logout cannot refill a new login session', async () => {
  const vm=require('node:vm');
  const fs=require('node:fs');
  let release,started,requests=0;
  const requestStarted=new Promise(resolve=>started=resolve);
  const id='b'.repeat(40);
  const context={URL,Headers,AbortController,TextDecoder,Uint8Array,setTimeout,clearTimeout,
    location:new URL('https://muse.ai/'),
    document:{readyState:'complete',body:{innerText:'Visible app',querySelectorAll:()=>[]},querySelectorAll:selector=>selector==='script[src]'?[{src:'https://muse.ai/_next/static/test.js'}]:[]},
    fetch:async(url,options)=>{
      if(String(url).endsWith('.js')) return new Response(`createServerReference("${id}",x,"fetchSubscriptionAction")`);
      if (++requests > 1) return new Response('{}');
      started(); return new Promise(resolve=>release=resolve);
    },getComputedStyle:()=>({display:'block',visibility:'visible',opacity:'1'})};
  context.window=context;context.top=context;
  vm.runInNewContext(fs.readFileSync(require.resolve('../assets/muse-observer.js'),'utf8'),withBrowserEvents(context));
  context.__museDesktopObserver.snapshot(true);
  await requestStarted;
  context.location=new URL('https://muse.ai/login');
  context.__museDesktopObserver.snapshot();
  context.location=new URL('https://muse.ai/');
  context.__museDesktopObserver.snapshot();
  release(new Response(JSON.stringify(sample({state:'AVAILABLE',percentUsed:28}))));
  await new Promise(resolve=>setImmediate(resolve));
  await new Promise(resolve=>setImmediate(resolve));
  assert.equal(context.__museDesktopObserver.snapshot().quota,null);
});
test('lazy settings loading discovers and calls only the named subscription reader', async () => {
  const vm=require('node:vm'),fs=require('node:fs');
  const scripts=[{src:'https://muse.ai/_next/static/app.js'}];
  let registered,loaderCalls=0,lookups=0;
  const id='c'.repeat(40);
  const context={URL,Headers,AbortController,TextDecoder,Uint8Array,setTimeout,clearTimeout,
    location:new URL('https://muse.ai/'),
    document:{readyState:'complete',body:{innerText:'Visible app',querySelectorAll:()=>[]},querySelectorAll:selector=>selector==='script[src]'?scripts:[]},
    fetch:async(url,options)=>{
      if(String(url).endsWith('/app.js')) return new Response('x.A(12345).then(({preloadHatchSettingsData:e})=>e)');
      if(String(url).endsWith('/settings.js')) return new Response(`createServerReference("${id}",x,"fetchSubscriptionAction")`);
      lookups++;assert.equal(options.headers['Next-Action'],id);
      assert.equal(options.body,'[{"includeAgreement":true}]');
      return new Response(JSON.stringify(sample({state:'AVAILABLE',percentUsed:46})));
    },getComputedStyle:()=>({display:'block',visibility:'visible',opacity:'1'})};
  context.window=context;context.top=context;
  context.TURBOPACK={push(chunk){
    if(typeof chunk[2]==='function') registered=chunk[2];
    else registered({A:async moduleId=>{assert.equal(moduleId,12345);loaderCalls++;scripts.push({src:'https://muse.ai/_next/static/settings.js'});}});
  }};
  vm.runInNewContext(fs.readFileSync(require.resolve('../assets/muse-observer.js'),'utf8'),withBrowserEvents(context));
  context.__museDesktopObserver.snapshot(true);
  for(let i=0;i<8;i++) await new Promise(resolve=>setImmediate(resolve));
  assert.equal(context.__museDesktopObserver.snapshot().quota.used_percent,46);
  assert.equal(loaderCalls,1);assert.equal(lookups,1);
});

test('avatar clip records changing frames with a bounded local timer and clears on logout', () => {
  const vm=require('node:vm'),fs=require('node:fs');
  let tick,cleared=false,frame=0,requests=0;
  const media={naturalWidth:72,naturalHeight:72,getBoundingClientRect:()=>({width:40,height:40}),closest:()=>null};
  const context={URL,Headers,AbortController,TextDecoder,Uint8Array,setTimeout,clearTimeout,
    setInterval:callback=>{tick=callback;return 1;},clearInterval:()=>{cleared=true;},
    location:new URL('https://muse.ai/'),
    document:{readyState:'complete',body:{innerText:'Visible app',querySelectorAll:()=>[]},
      querySelectorAll:selector=>selector.includes('avatar-host]')?[media]:[],
      createElement:()=>({getContext:()=>({drawImage(){}}),toDataURL:()=>`data:image/png;base64,${++frame}`})},
    fetch:async()=>{requests++;throw new Error('no network required for avatar');},
    getComputedStyle:()=>({display:'block',visibility:'visible',opacity:'1'})};
  context.window=context;context.top=context;
  vm.runInNewContext(fs.readFileSync(require.resolve('../assets/muse-observer.js'),'utf8'),withBrowserEvents(context));
  context.__museDesktopObserver.snapshot();
  assert.equal(typeof tick,'function');
  for(let i=0;i<60;i++)tick();
  const clip=context.__museDesktopObserver.snapshot().avatar_animation;
  assert.equal(clip.frames.length,48);
  assert.equal(clip.frame_ms,125);
  assert.notEqual(clip.frames[0],clip.frames[47]);
  assert.equal(cleared,true);
  assert.equal(requests,0);
  assert.equal(context.__museDesktopObserver.snapshot().avatar_animation,null,'finished clip is sent once');
  context.__museDesktopObserver.snapshot(true);
  for(let i=0;i<48;i++)tick();
  const updated=context.__museDesktopObserver.snapshot().avatar_animation;
  assert.ok(updated,'manual refresh records a fresh avatar instead of keeping the first clip forever');
  assert.notEqual(updated.frames[0],clip.frames[0]);
  context.location=new URL('https://muse.ai/login');
  assert.equal(context.__museDesktopObserver.snapshot().avatar_png,null);
  assert.equal(context.__museDesktopObserver.snapshot().avatar_animation,null);
});

test('avatar recording stops when a document leaves the trusted Muse origin', () => {
  const vm=require('node:vm'),fs=require('node:fs');let tick,cleared=false;
  const media={naturalWidth:72,naturalHeight:72,getBoundingClientRect:()=>({width:40,height:40}),closest:()=>null};
  const context={URL,Headers,AbortController,TextDecoder,Uint8Array,setTimeout,clearTimeout,
    setInterval:callback=>{tick=callback;return 1;},clearInterval:()=>{cleared=true;},location:new URL('https://muse.ai/'),
    document:{readyState:'complete',body:{innerText:'Visible app',querySelectorAll:()=>[]},querySelectorAll:s=>s.includes('avatar-host]')?[media]:[],
      createElement:()=>({getContext:()=>({drawImage(){}}),toDataURL:()=>`data:image/png;base64,AA==`})},
    fetch:async()=>{throw new Error('no requests');},getComputedStyle:()=>({display:'block',visibility:'visible',opacity:'1'})};
  context.window=context;context.top=context;
  vm.runInNewContext(fs.readFileSync(require.resolve('../assets/muse-observer.js'),'utf8'),withBrowserEvents(context));
  context.__museDesktopObserver.snapshot();
  context.location=new URL('https://example.test/');tick();
  assert.equal(cleared,true);
  assert.equal(context.__museDesktopObserver.snapshot().avatar_animation,null);
});
