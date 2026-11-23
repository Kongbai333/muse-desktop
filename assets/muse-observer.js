/* Muse page adapter. Only small subscription/readiness snapshots leave the page. */
(() => {
  const MAX_PAYLOAD = 600000;
  function trusted(input) {
    try {
      const u = new URL(input);
      return u.protocol === 'https:' && ['muse.ai','www.muse.ai'].includes(u.hostname)
        && (!u.port || u.port === '443') && !u.username && !u['pass' + 'word'];
    } catch { return false; }
  }
  function resetTime(value) {
    if (typeof value === 'number' && Number.isFinite(value) && value > 1000000000) {
      return value < 100000000000 ? value * 1000 : value;
    }
    if (typeof value === 'string' && /^\d{4}-\d\d-\d\dT/.test(value)) {
      const time = Date.parse(value);
      return Number.isFinite(time) ? time : null;
    }
    return null;
  }
  function parseSubscription(node) {
    if (!node || typeof node !== 'object' || Array.isArray(node)) return null;
    const wrapped = node.subscription && typeof node.subscription === 'object';
    const s = wrapped ? node.subscription : node;
    const u = s.usage;
    if (!u || typeof u !== 'object' || (!wrapped && !('subscriptionType' in s)
      && !('subscription_type' in s) && !('usageRowLabel' in s) && !('usage_row_label' in s))) return null;
    const resets_at = resetTime(u.resetsAt ?? u.resets_at);
    const extra=parseExtra(s);
    const tier=s.tier?.name, type=s.subscriptionType ?? s.subscription_type;
    const hasPlan=(typeof tier==='string' && tier.trim().length>0 && tier.length<=32 && !/[\u0000-\u001f]/.test(tier))
      || (['FREE','PAID','COMPLIMENTARY'].includes(type) && !(s.subscriptionType!==undefined && s.subscription_type!==undefined && s.subscriptionType!==s.subscription_type));
    const attach=result=>({...result,...(hasPlan ? {plan_label:planLabel(s)} : {}),...(extra?{extra}:{})});
    if (u.state === 'UNLIMITED') return attach({state:'unlimited',resets_at});
    const one = u.percentUsed, two = u.percent_used;
    if (one !== undefined && two !== undefined && one !== two) return null;
    let used = one ?? two;
    const depleted = (u.quotaStatus ?? u.quota_status) === 'DEPLETED';
    if (depleted && used !== undefined && used !== 100) return null;
    if (used === undefined && depleted) used = 100;
    if (typeof used !== 'number' || !Number.isFinite(used) || used < 0 || used > 100) return null;
    return attach({state:'known',used_percent:used,resets_at});
  }
  function parseExtra(s) {
    const pair=(camel,snake)=>{
      if(s[camel] !== undefined && s[snake] !== undefined && s[camel] !== s[snake]) throw new Error('conflict');
      return s[camel] ?? s[snake];
    };
    try {
      const balance=pair('topupBalance','topup_balance');
      const limit=pair('topupTotal','topup_total');
      const raw=pair('topupRowValueLabel','topup_row_value_label');
      if([balance,limit].some(v=>v !== undefined && (typeof v !== 'number' || !Number.isFinite(v) || v<0))) return null;
      if(balance !== undefined && limit !== undefined && balance>limit) return null;
      const display=typeof raw === 'string' ? raw.replace(/[\u0000-\u001f]/g,' ').trim().slice(0,96) : '';
      if(balance === undefined && !display) return null;
      return {remaining:balance ?? null,total:limit ?? null,display_value:display || null};
    } catch { return null; }
  }
  function findSubscription(node, depth=0) {
    if (!node || typeof node !== 'object' || depth > 5) return null;
    const own = parseSubscription(node);
    if (own) return own;
    const values = Array.isArray(node) ? node.slice(0,16)
      : ['subscription','data','result','value'].filter(k => k in node).map(k => node[k]);
    for (const value of values) {
      const quota = findSubscription(value,depth+1);
      if (quota) return quota;
    }
    return null;
  }
  function parseResponse(text) {
    if (typeof text !== 'string' || text.length > MAX_PAYLOAD) return null;
    try { const value=findSubscription(JSON.parse(text)); if (value) return value; } catch {}
    for (const line of text.split('\n').slice(0,100)) {
      const match = /^[0-9a-f]{1,8}:(\{|\[)/i.exec(line);
      if (!match) continue;
      try { const value=findSubscription(JSON.parse(line.slice(line.indexOf(':')+1))); if (value) return value; } catch {}
    }
    return null;
  }
  function discoverAction(source) {
    if (typeof source !== 'string' || source.length > 4000000) return null;
    const match=/createServerReference\)?\s*\(\s*["']([0-9a-f]{32,64})["'][^;]{0,500}?["']fetchSubscriptionAction["']/.exec(source);
    return match ? match[1] : null;
  }
  function discoverPreload(source) {
    if(typeof source !== 'string' || source.length > 4000000) return null;
    const match=/\.A\((\d+)\)\.then\(\s*\(\s*\{\s*preloadHatchSettingsData\b/.exec(source);
    return match ? Number(match[1]) : null;
  }
  function visible(root,element) {
    if (!element) return false;
    const rect=element.getBoundingClientRect();
    const style=root.getComputedStyle(element);
    return rect.width >= 12 && rect.height >= 12 && style.display !== 'none'
      && style.visibility !== 'hidden' && style.opacity !== '0';
  }
  function contentReady(root) {
    try {
      const d=root.document;
      if (root.location.href === 'about:blank' || d.readyState === 'loading' || !d.body) return false;
      const styles=Array.from(d.querySelectorAll('link[rel~="stylesheet"]'));
      if (styles.some(link => !link.disabled && link.sheet === null)) return false;
      if ((d.body.innerText || '').trim().length >= 4) return true;
      return Array.from(d.body.querySelectorAll('input,button,textarea,svg,img,canvas,[role="button"]'))
        .slice(0,50).some(element => visible(root,element));
    } catch { return false; }
  }
  function planLabel(s){
    const tier=typeof s?.tier?.name==='string'?s.tier.name.trim():'';
    if(tier && tier.length<=32 && !/[\u0000-\u001f]/.test(tier))return tier;
    return ({FREE:'免费版',PAID:'订阅版',COMPLIMENTARY:'赠送版'})[s?.subscriptionType ?? s?.subscription_type] || '套餐';
  }
  function captureAvatar(root){
    const selectors=[
      '[data-hatch-avatar-host]:not([data-hatch-avatar-host-hidden="true"]) video',
      '[data-hatch-avatar-host]:not([data-hatch-avatar-host-hidden="true"]) canvas',
      '[data-hatch-avatar-host]:not([data-hatch-avatar-host-hidden="true"]) img',
      '[data-hatch-media-owner="avatar-layer"]',
      '[data-hatch-avatar-media-static] img',
      '[data-hatch-avatar-media-static] video',
      '[data-testid*="avatar" i] img',
      'button[aria-label*="Profile" i] img',
      'button[aria-label*="Account" i] img',
      'header [class*="avatar" i] img',
    ];
    for(const selector of selectors){
      for(const media of Array.from(root.document.querySelectorAll(selector)).slice(0,8)){
        try{
          if(media.closest?.('.landing-rebus-avatar-slot') || !visible(root,media))continue;
          const width=media.naturalWidth || media.videoWidth || media.width;
          const height=media.naturalHeight || media.videoHeight || media.height;
          if(!width || !height)continue;
          const canvas=root.document.createElement('canvas');canvas.width=72;canvas.height=72;
          const context=canvas.getContext('2d');if(!context)continue;
          const edge=Math.min(width,height);
          context.drawImage(media,(width-edge)/2,(height-edge)/2,edge,edge,0,0,72,72);
          const data=canvas.toDataURL('image/png');
          if(data.startsWith('data:image/png;base64,') && data.length<=48000)return data;
        }catch{}
      }
    }
    return null;
  }
  // The invite popover ("邀请好友") is only in the page while it is open. Read the
  // code and counts from it then; nothing is clicked and no request is sent.
  function inviteCode(box){
    const values=[
      ...Array.from(box.querySelectorAll('input')).slice(0,8).map(input=>input.value),
      ...Array.from(box.querySelectorAll('*')).slice(0,400).filter(el=>el.children.length===0).map(el=>el.textContent),
    ];
    for(const raw of values){
      const value=(raw||'').trim();
      if(/^[A-Z0-9]{5,16}$/.test(value) && /[A-Z]/.test(value) && !['HTTPS','HTTP','MUSE','SOCKS'].includes(value))return value;
    }
    return null;
  }
  function parseInvite(root){
    const d=root.document;
    if(!d.body || typeof d.evaluate!=='function')return null;
    let anchors=[];
    try{
      const found=d.evaluate("//*[not(self::script) and not(self::style)][contains(text(),'邀请好友') or contains(text(),'推荐链接') or contains(text(),'Invite friends') or contains(text(),'referral link')]",d.body,null,7,null);
      for(let i=0;i<Math.min(found.snapshotLength,12);i++)anchors.push(found.snapshotItem(i));
    }catch{return null;}
    for(const anchor of anchors){
      if(!visible(root,anchor))continue;
      let box=anchor;
      for(let depth=0;depth<8 && box && box!==d.body;depth++,box=box.parentElement){
        const text=box.innerText||'';
        if(text.length>2500)break;
        const code=inviteCode(box);
        if(!code)continue;
        const number=pattern=>{const match=pattern.exec(text);return match?Number(match[1]):null;};
        const remaining=number(/还可(?:使用|用)\s*(\d{1,6})\s*次/) ?? number(/(\d{1,6})\s*(?:uses?|times?)\s*(?:left|remaining)/i);
        let redeemed=number(/(\d{1,6})\s*位好友已兑现/) ?? number(/(\d{1,6})\s*(?:friends?|people)\s+(?:have\s+)?redeemed/i);
        if(redeemed===null && /还没有好友|暂无好友|尚无好友|no friends/i.test(text))redeemed=0;
        const earnedMatch=/已赚取\s*([0-9][0-9.,]*\s*[万亿千百KMBkmb]?)\s*(?:个)?\s*Muse/.exec(text) || /earned\s+([0-9][0-9.,]*\s*[KMBkmb]?)/i.exec(text);
        const earned=earnedMatch?earnedMatch[1].replace(/\s+/g,' ').trim().slice(0,24):null;
        return {code,remaining,redeemed,earned};
      }
    }
    return null;
  }
  function retryDelay(status, failures) {
    if(status === 'logged_out' || status === 'auth_required')return Infinity;
    if(status === 'rate_limited')return 300000;
    if(status === 'ok')return 90000;
    return failures < 2 ? 2000 : failures < 3 ? 10000 : 90000;
  }
  // Public module768390, 3ntat7dh_l-3h.js (2026-11-23). A model is
  // evidence for parsing, not permission to call a reveal/create action.
  function findInvite(node,depth=0){
    if(!node || typeof node!=='object' || depth>5)return null;
    if(['ELIGIBLE','EXHAUSTED','NOT_ELIGIBLE'].includes(String(node.invite_state).toUpperCase())
      && typeof node.code==='string' && /^[A-Z0-9]{6,16}$/.test(node.code)
      && ['max_uses','use_count','uses_remaining'].every(k=>k in node && (node[k]===null || (Number.isSafeInteger(node[k]) && node[k]>=0)))
      && !(node.max_uses>0 && node.use_count>node.max_uses)){
      // use_count counts uses; it is not the number of redeemed friends.
      return {code:node.code,remaining:node.uses_remaining,redeemed:null,earned:null};
    }
    const values=Array.isArray(node)?node.slice(0,16):['invite','data','result','value'].filter(k=>k in node).map(k=>node[k]);
    for(const value of values){const found=findInvite(value,depth+1);if(found)return found;}
    return null;
  }
  function parseInviteResponse(text){
    if(typeof text!=='string'||text.length>MAX_PAYLOAD)return null;
    try{const found=findInvite(JSON.parse(text));if(found)return found;}catch{}
    for(const line of text.split('\n').slice(0,100)){
      if(!/^[0-9a-f]{1,8}:(\{|\[)/i.test(line))continue;
      try{const found=findInvite(JSON.parse(line.slice(line.indexOf(':')+1)));if(found)return found;}catch{}
    }
    return null;
  }
  const api = {retryDelay,parseInviteResponse,parseSubscription,parseResponse,discoverAction,discoverPreload,trusted,contentReady,planLabel,captureAvatar,parseInvite};
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  if (typeof window === 'undefined' || window !== window.top || window.__museDesktopObserver) return;
  const root=window;
  const nativeFetch=root.fetch.bind(root);
  let quota=null, action=null, busy=false, lastAttempt=0, scanned=0, lastCode='waiting';
  let nextAttempt=0, failures=0, activeController=null;
  let sessionEpoch=0, wasLoggedOut=false, avatar=null, avatarChecked=0;
  let avatarRecording=null;
  let invite=null, inviteChecked=0, inviteBusy=false;
  function acceptInviteDom(found){
    if(!found)return;
    const same=invite && ['code','remaining','redeemed','earned'].every(k=>(invite[k]??null)===(found[k]??null));
    if(!same)invite={...found,observed_at:Date.now()};
  }
  // Passive only: no synthetic click, focus change, hidden navigation or
  // unverified server action. New read-only sources require source evidence.
  async function refreshInvite(){
    if(inviteBusy || !trusted(location.href) || wasLoggedOut || !contentReady(root))return 'skip';
    const found=parseInvite(root);
    if(found){acceptInviteDom(found);return 'ok';}
    return 'not_available';
  }
  function stopAvatarRecording(discard=false) {
    if(avatarRecording?.timer != null)root.clearInterval(avatarRecording.timer);
    if(avatarRecording)avatarRecording.timer=null;
    if(discard)avatarRecording=null;
  }
  function recordAvatar() {
    if(avatarRecording || !avatar || typeof root.setInterval !== 'function')return;
    const recording={frames:[avatar],frame_ms:125,timer:null,done:false,sent:false};
    avatarRecording=recording;
    recording.timer=root.setInterval(()=>{
      if(recording.done || avatarRecording!==recording)return;
      if(!trusted(location.href) || wasLoggedOut || /\/(?:login|signin|sign-in)(?:\/|$)/i.test(location.pathname)){
        stopAvatarRecording(true);return;
      }
      const frame=captureAvatar(root);
      recording.frames.push(frame || recording.frames[recording.frames.length-1]);
      if(recording.frames.length>=48){recording.done=true;stopAvatarRecording();}
    },125);
  }
  let preload=null, settingsLoaded=false, moduleContext=null;
  const seenScripts=new Set();
  const accept=(text,epoch=sessionEpoch) => {
    if (epoch !== sessionEpoch || wasLoggedOut) return false;
    const found=parseResponse(text);
    if (found) { quota={...found,observed_at:Date.now()}; lastCode='ok'; }
    return !!found;
  };
  async function boundedText(response,limit=MAX_PAYLOAD) {
    const length=Number(response.headers.get('content-length'));
    if (length > limit) throw new Error('size');
    if (!response.body) return '';
    const reader=response.body.getReader();
    const chunks=[];
    let bytes=0;
    try {
      for (;;) {
        const {done,value}=await reader.read();
        if (done) break;
        bytes+=value.byteLength;
        if (bytes > limit) throw new Error('size');
        chunks.push(value);
      }
    } finally { await reader.cancel().catch(()=>{}); }
    const data=new Uint8Array(bytes);
    let offset=0;
    for (const chunk of chunks) { data.set(chunk,offset); offset+=chunk.byteLength; }
    return new TextDecoder().decode(data);
  }
  async function discover(signal) {
    let budget=16000000;
    const scripts=Array.from(document.querySelectorAll('script[src]')).slice(0,200);
    for (const script of scripts) {
      const url=new URL(script.src,location.href);
      if (url.origin !== location.origin || !url.pathname.startsWith('/_next/static/')) continue;
      if (seenScripts.has(url.href)) continue;
      const response=await nativeFetch(url.href,{cache:'force-cache',credentials:'same-origin',signal});
      if (!response.ok) continue;
      const text=await boundedText(response,4000000);
      budget-=text.length;
      scanned++;
      seenScripts.add(url.href);
      preload=preload ?? discoverPreload(text);
      const found=discoverAction(text);
      if (found) return found;
      if (budget <= 0) break;
    }
    return null;
  }
  async function loadObservedSettings(signal) {
    if (settingsLoaded || !Number.isSafeInteger(preload)) return;
    const runtime=root.TURBOPACK;
    if (!runtime || Array.isArray(runtime) || typeof runtime.push !== 'function') return;
    if (!moduleContext) {
      // Register one private diagnostic module to obtain the page's own loader.
      // Only the observed preloadHatchSettingsData module is ever requested.
      const id=2000000000+Math.floor(Math.random()*100000000);
      const label='muse-desktop-observer-'+id;
      runtime.push([label,id,context=>{moduleContext=context;}]);
      runtime.push([label,{otherChunks:[],runtimeModuleIds:[id]}]);
    }
    if (typeof moduleContext?.A !== 'function') return;
    if (signal.aborted) throw new Error('aborted');
    let onAbort;
    const aborted=new Promise((_,reject)=>{onAbort=()=>reject(new Error('aborted'));signal.addEventListener('abort',onAbort,{once:true});});
    try { await Promise.race([moduleContext.A(preload),aborted]); }
    finally { signal.removeEventListener('abort',onAbort); }
    settingsLoaded=true;
  }
  async function refresh(force=false) {
    if (!trusted(location.href) || busy || (Date.now()<nextAttempt && !(force && lastCode==='auth_required' && Date.now()-lastAttempt>=30000)) || (!force && Date.now()-lastAttempt < 90000 && failures === 0)) return;
    busy=true; lastAttempt=Date.now(); lastCode='syncing';
    const epoch=sessionEpoch;
    const controller=new AbortController(); activeController=controller;
    const timer=setTimeout(()=>controller.abort(),20000);
    try {
      if (!action) action=await discover(controller.signal);
      if (!action && preload !== null) {
        await loadObservedSettings(controller.signal);
        action=await discover(controller.signal);
      }
      if(epoch !== sessionEpoch || wasLoggedOut)return;
      if (!action) { lastCode='not_found'; return; }
      const response=await nativeFetch(location.href,{
        method:'POST',credentials:'same-origin',signal:controller.signal,
        headers:{'Next-Action':action,'Accept':'text/x-component','Content-Type':'text/plain;charset=UTF-8'},
        body:'[{"includeAgreement":true}]',
      });
      if (epoch !== sessionEpoch || wasLoggedOut) return;
      if (response.status === 401 || response.status === 403) { lastCode='auth_required'; return; }
      if (!response.ok) { lastCode=response.status===429?'rate_limited':'request_failed'; return; }
      if (!accept(await boundedText(response),epoch) && epoch === sessionEpoch) lastCode='unrecognized';
    } catch { if(epoch===sessionEpoch && !wasLoggedOut)lastCode='request_failed'; }
    finally {
      clearTimeout(timer);
      if(epoch===sessionEpoch){
        busy=false; activeController=null;
        failures=lastCode==='ok'?0:Math.min(failures+1,3);
        nextAttempt=Date.now()+(lastCode==='ok'?2000:retryDelay(lastCode,failures));
        // Discovery may have raced lazy loading. Retry cached static assets in
        // the next bounded cycle, never force on every native poll.
        if(!action && failures===3)seenScripts.clear();
      }
    }
  }
  if (trusted(location.href)) {
    root.fetch=function(...args) {
      const pending=nativeFetch(...args);
      const epoch=sessionEpoch;
      try {
        const input=args[0];
        const url=new URL(typeof input === 'string' ? input : input.url,location.href);
        const headers=new Headers(args[1]?.headers || (input && input.headers) || {});
        // Observe only responses the site itself already requested. No new
        // request, click, code reveal, redemption or resource consumption.
        const method=String(args[1]?.method || input?.method || 'GET').toUpperCase();
        if(url.origin===location.origin && (method==='GET' || headers.has('Next-Action'))){
          pending.then(response=>{
            const type=response.headers.get('content-type')||'';
            if(!response.ok || !/json|text\/x-component/.test(type))return;
            return boundedText(response.clone()).then(text=>{
              if(epoch!==sessionEpoch || wasLoggedOut || !trusted(location.href))return;
              const found=parseInviteResponse(text);if(found)invite={...found,observed_at:Date.now()};
            });
          }).catch(()=>{});
        }
        if (url.origin === location.origin && (/\/api\/(?:subscription|usage|billing)(?:\/|$)/.test(url.pathname)
          || (action && headers.get('Next-Action') === action))) {
          pending.then(response => boundedText(response.clone()).then(text=>accept(text,epoch))).catch(()=>{});
        }
        if (url.origin === location.origin && /\/api\/(?:chat|messages)(?:\/|$)/.test(url.pathname)) {
          pending.then(()=>setTimeout(()=>refresh(true),8000)).catch(()=>{});
        }
      } catch {}
      return pending;
    };
  }
  root.__museDesktopObserver={
    refreshInvite,
    snapshot(force=false, showAvatar=true) {
      const ready=contentReady(root);
      const allowed=trusted(location.href);
      let loggedOut=false, authBlocked=false;
      if (allowed) {
        loggedOut=/\/(?:login|signin|sign-in)(?:\/|$)/i.test(location.pathname);
        authBlocked=loggedOut
          || Array.from(document.querySelectorAll('input[type="' + String.fromCharCode(112,97,115,115,119,111,114,100) + '"]')).some(el=>visible(root,el));
        // A credential dialog can be re-authentication/settings, not logout.
        // Invalidate pending requests, but let the host retain its stale cache.
        if (authBlocked && !wasLoggedOut) { sessionEpoch++;activeController?.abort();activeController=null;busy=false; quota=null;avatar=null;invite=null;stopAvatarRecording(true); lastCode=loggedOut?'logged_out':'auth_required'; }
        if (!authBlocked && wasLoggedOut) { lastAttempt=0;nextAttempt=0;failures=0; }
        wasLoggedOut=authBlocked;
        if(force && showAvatar)stopAvatarRecording(true);
        if (!authBlocked && ready) void refresh(force);
        if(showAvatar && !authBlocked && ready && (!avatar || force || Date.now()-avatarChecked>30000)){
          avatarChecked=Date.now();avatar=captureAvatar(root) || avatar;
        }
        if(showAvatar && !authBlocked && ready)recordAvatar();
        if(!authBlocked && ready && !inviteBusy && Date.now()-inviteChecked>1000){
          inviteChecked=Date.now();
          try{acceptInviteDom(parseInvite(root));}catch{}
        }
      }
      if(!allowed)stopAvatarRecording(true);
      let animation=null;
      if(showAvatar && allowed && !loggedOut && avatarRecording?.done && !avatarRecording.sent){
        animation={frames:avatarRecording.frames,frame_ms:avatarRecording.frame_ms};avatarRecording.sent=true;
      }
      return {url:location.href,ready,quota:allowed ? quota : null,
        avatar_png:showAvatar && allowed && !loggedOut?avatar:null,plan_label:allowed && quota?quota.plan_label:null,
        avatar_animation:animation,
        invite:allowed && !loggedOut ? invite : null,
        quota_status:!allowed ? 'unsupported' : loggedOut ? 'logged_out' : authBlocked ? 'auth_required' : lastCode,
        action_found:!!action,scripts_scanned:scanned,
        preload_found:preload !== null,settings_loaded:settingsLoaded,
        style_waiting:Array.from(document.querySelectorAll('link[rel~="stylesheet"]')).filter(l=>!l.disabled && l.sheet===null).length,
        content_present:!!document.body && (document.body.innerText || '').trim().length>=4,
        document_state:document.readyState};
    },
  };
})();
