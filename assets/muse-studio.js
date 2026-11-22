// Account-page adapter. Selector vocabulary informed by MUSE2API (MIT),
// see THIRD_PARTY_NOTICES.md. No cookies or remote API keys leave this page.
(() => {
  if(window!==window.top || window.__museStudio)return;
  const trusted=()=>['https://muse.ai','https://www.muse.ai'].includes(location.origin);
  const bubbles=()=>Array.from(document.querySelectorAll('div[class*="hatch-chat-groupable-bubble"]'));
  const agents=()=>bubbles().filter(e=>/hatch-agent-bubble-bg/.test(e.className||''));
  const users=()=>bubbles().filter(e=>/chat-user-bubble/.test(e.className||''));
  const visible=e=>!!e && e.getClientRects().length>0 && getComputedStyle(e).visibility!=='hidden';
  const attachments=()=>Array.from(document.querySelectorAll('[data-testid^="hatch-chat-attachment-presentation-"]'));
  const busy=()=>!!document.querySelector('[data-testid="hatch-composer-stop-button"],button[aria-label*="Stop" i],button[aria-label*="停止"]');
  let job=null;
  const result=(state,extra={})=>JSON.stringify({id:job?.id,state,...extra});
  function sendButton(ta){
    for(let root=ta.parentElement;root && root!==document.body;root=root.parentElement){
      const button=Array.from(root.querySelectorAll('button,[role="button"]')).find(b=>visible(b)
        && !b.closest('aside,nav,[role="navigation"],div[class*="chat-user-bubble"],div[class*="group/msg"],div[class*="hatch-chat-groupable-bubble"]')
        && (/^(?:hatch-composer-send-button|composer-send-button|send-button)$/i.test(b.dataset.testid||'')
          || /^(?:send(?:\s+message)?|发送(?:消息)?|發送(?:訊息)?|傳送(?:訊息)?|送出)$/i.test((b.getAttribute('aria-label')||b.innerText||'').trim())));
      if(button)return button;
      if(root.matches('form,main,[role="main"]'))break;
    }return null;
  }
  function scrollBottom(){
    const ta=document.querySelector('textarea');
    const root=ta?.closest('main') || document.querySelector('main');
    if(root)for(const e of root.querySelectorAll('div')) {
      if(e.scrollHeight>e.clientHeight+100 && /auto|scroll/.test(getComputedStyle(e).overflowY)){e.scrollTop=e.scrollHeight;}
    }
  }
  const sourceOf=element=>element.currentSrc||element.src||'';
  const sizeOf=element=>({width:element.naturalWidth||element.videoWidth||0,height:element.naturalHeight||element.videoHeight||0});
  const attachmentId=element=>element.closest('[data-testid^="hatch-chat-attachment-presentation-"]')?.getAttribute('data-testid')||'';
  const wait=milliseconds=>new Promise(resolve=>setTimeout(resolve,milliseconds));
  let upload=null;
  function uploadResponse(id,state,extra={}){return JSON.stringify({id,state,...extra});}
  // Composer-scoped lookup (MUSE2API engine vocabulary): the new composer has no <form>,
  // so fall back to the textarea's nearest ancestors before searching the whole page.
  const bubbleSelector='[class*="chat-user-bubble"],[class*="group/msg"],[class*="hatch-chat-groupable-bubble"]';
  const removeStrict=/remove\s*(?:attachment|file|image)|移除附件|删除附件|刪除附件|移除檔案|移除附檔|移除图片|删除图片/i;
  const removeLoose=/remove|移除|删除|刪除/i;
  function composerRoots(){
    try{
      const overlay=document.querySelector('[data-testid="hatch-composer-placeholder-overlay"]');
      const legacy=overlay?(overlay.closest('form')||overlay.parentElement?.parentElement?.parentElement):null;
      if(legacy)return [legacy];
      const ta=Array.from(document.querySelectorAll('textarea')).find(visible)||document.querySelector('textarea');
      if(!ta)return [];
      const form=ta.closest&&ta.closest('form');
      if(form)return [form];
      const roots=[];let element=ta.parentElement;
      for(let index=0;index<4&&element&&element!==document.body;index++){roots.push(element);element=element.parentElement;}
      return roots;
    }catch{return [];}
  }
  const acceptsImages=candidate=>{const accept=(candidate.accept||'').trim();
    return !accept||/image\/|\*|\.(?:jpe?g|png|webp|gif|avif|heic|heif)/i.test(accept);};
  function fileInput(){
    const inputs=Array.from(document.querySelectorAll('input[type="file"]')).filter(candidate=>!candidate.disabled&&acceptsImages(candidate));
    for(const root of composerRoots()){const hit=inputs.find(candidate=>root.contains&&root.contains(candidate));if(hit)return hit;}
    // Only trust a page-wide input when it is the single candidate; otherwise it may belong to another feature.
    return inputs.length===1?inputs[0]:null;
  }
  function removeButtons(pattern){
    for(const root of composerRoots()){
      const found=Array.from(root.querySelectorAll('button[aria-label]')).filter(button=>pattern.test(button.getAttribute('aria-label')||'')
        &&!(button.closest&&button.closest(bubbleSelector)));
      if(found.length)return found;
    }
    return [];
  }
  function previewCount(){
    for(const root of composerRoots()){
      const sources=new Set();
      for(const image of Array.from(root.querySelectorAll('img'))){
        if(image.closest&&image.closest(bubbleSelector))continue;
        const src=sourceOf(image),rect=image.getBoundingClientRect?image.getBoundingClientRect():{width:40,height:40};
        if(src&&rect.width>=16&&rect.height>=16)sources.add(src);
      }
      if(sources.size)return sources.size;
    }
    return 0;
  }
  const composerCount=()=>Math.max(removeButtons(removeStrict).length,previewCount());
  function diagnose(){
    try{
      const roots=composerRoots(),ta=document.querySelector('textarea');
      const inputs=Array.from(document.querySelectorAll('input[type="file"]')).map(input=>(input.accept||'任意')
        +(roots.some(root=>root.contains(input))?'@输入区':'')+(input.disabled?'(禁用)':'')+'='+(input.files?input.files.length:0));
      const labels=[];
      for(const root of roots.slice(0,2))for(const button of Array.from(root.querySelectorAll('button,[role="button"]'))){
        const label=(button.getAttribute('aria-label')||button.innerText||'').trim().replace(/\s+/g,' ').slice(0,16);
        if(label&&!labels.includes(label))labels.push(label);
      }
      return ('输入框:'+(ta?'有':'无')+'；上传控件:'+(inputs.join('，')||'无')+'；输入区按钮:'+(labels.slice(0,12).join('|')||'无')
        +'；方式:'+(confirm?['input','drop','paste'][confirm.stage]:'-')).slice(0,420);
    }catch{return '';}
  }
  // Files handed to the composer during this task, kept for the drop / paste fallbacks.
  let sent=[],confirm=null;
  function inject(kind){
    const transfer=new DataTransfer();for(const file of sent)transfer.items.add(file);
    const ta=Array.from(document.querySelectorAll('textarea')).find(visible)||document.querySelector('textarea');
    try{
      if(kind==='drop'){
        const target=composerRoots()[0]||ta;if(!target)return;
        for(const type of ['dragenter','dragover','drop'])
          target.dispatchEvent(new DragEvent(type,{bubbles:true,cancelable:true,dataTransfer:transfer}));
      }else if(ta){
        ta.focus&&ta.focus();
        ta.dispatchEvent(new ClipboardEvent('paste',{bubbles:true,cancelable:true,clipboardData:transfer}));
      }
    }catch{}
  }
  function attachedCount(id,expected){
    if(!trusted())return uploadResponse(id,'error',{error:'请先在 Muse 登录'});
    const count=composerCount(),now=Date.now(),want=Number(expected)||sent.length;
    if(!confirm||confirm.id!==id)confirm={id,stage:0,at:now};
    // Synthetic change events are ignored by some upload widgets; try drop, then paste, only when nothing arrived.
    if(count===0&&sent.length&&confirm.stage<2&&now-confirm.at>3500){confirm.stage++;confirm.at=now;inject(confirm.stage===1?'drop':'paste');}
    return uploadResponse(id,'attachments',{count,expected:want,method:['input','drop','paste'][confirm.stage],diag:count<want?diagnose():''});
  }
  function uploadChunk(id,offset,data,eof){
    if(!trusted())return uploadResponse(id,'error',{error:'请先在 Muse 登录'});
    if(offset===0)upload={id,offset:0,parts:[],complete:false,file:null};
    if(!upload||upload.id!==id||offset!==upload.offset)return uploadResponse(id,'error',{error:'参考图分块顺序错误'});
    let binary;
    try{binary=atob(data||'');}catch{return uploadResponse(id,'error',{error:'参考图数据无效'});}
    if(!binary.length||binary.length>24*1024||upload.offset+binary.length>8*1024*1024)
      return uploadResponse(id,'error',{error:'参考图大小不受支持'});
    const bytes=new Uint8Array(binary.length);for(let index=0;index<binary.length;index++)bytes[index]=binary.charCodeAt(index);
    upload.parts.push(bytes);upload.offset+=bytes.length;upload.complete=!!eof;
    return uploadResponse(id,eof?'uploaded':'uploading',{offset:upload.offset});
  }
  function commitUpload(id,name,mime,replace){
    if(!trusted())return uploadResponse(id,'error',{error:'请先在 Muse 登录'});
    if(!upload||upload.id!==id||!upload.complete)return uploadResponse(id,'error',{error:'参考图还没有上传完成'});
    if(!/^image\//.test(mime||''))return uploadResponse(id,'error',{error:'参考图必须是图片文件'});
    if(!upload.file){
      const total=upload.parts.reduce((size,part)=>size+part.length,0),all=new Uint8Array(total);let offset=0;
      for(const part of upload.parts){all.set(part,offset);offset+=part.length;}
      try{upload.file=new File([all],name||'reference.jpg',{type:mime});}
      catch{return uploadResponse(id,'error',{error:'当前 WebView 不支持参考图上传'});}
    }
    // First reference of a task: clear attachments left in the composer, then let React settle.
    if(replace&&!upload.cleared){
      upload.cleared=true;sent=[];confirm=null;
      const stale=removeButtons(removeLoose);
      for(const button of stale){try{button.click();}catch{}}
      if(stale.length)return uploadResponse(id,'locating',{cleared:stale.length});
    }
    const input=composerRoots().length?fileInput():null;
    // The composer may still be hydrating; the caller retries before giving up.
    if(!input)return uploadResponse(id,'locating',{error:'Muse 页面没有可用的图片上传控件，请刷新页面后重试'});
    try{
      const transfer=new DataTransfer();transfer.items.add(upload.file);input.files=transfer.files;
      input.dispatchEvent(new Event('input',{bubbles:true}));input.dispatchEvent(new Event('change',{bubbles:true}));
      sent.push(upload.file);
      const name=upload.file.name,type=upload.file.type;upload.parts=[];upload.file=null;upload.attached=true;
      return uploadResponse(id,'attached',{name,mime:type});
    }catch{return uploadResponse(id,'error',{error:'当前 WebView 不支持参考图上传'});}
  }
  async function mediaBytes(src,owner,maximum){
    const url=new URL(src,location.href);
    if(!['https:','blob:'].includes(url.protocol))throw new Error('invalid-media-url');
    const response=await fetch(url.href,{credentials:'include',signal:owner.controller.signal});
    if(!response.ok)throw new Error('media-download');
    if(Number(response.headers.get('content-length'))>maximum)throw new Error('media-too-large');
    const reader=response.body.getReader(),chunks=[];let size=0;
    try{for(;;){const {done,value}=await reader.read();if(done)break;size+=value.length;if(size>maximum)throw new Error('media-too-large');chunks.push(value);}}
    finally{await reader.cancel().catch(()=>{});}
    const bytes=new Uint8Array(size);let at=0;for(const chunk of chunks){bytes.set(chunk,at);at+=chunk.length;}
    return {bytes,mime:(response.headers.get('content-type')||'').split(';')[0].trim().toLowerCase()};
  }
  function closeViewer(element){
    const dialog=element?.closest('[role="dialog"],[aria-modal="true"]');
    const close=dialog?.querySelector('button[aria-label*="Close" i],button[aria-label*="关闭"]');
    if(close){close.click();return;}
    const target=dialog||document.activeElement||document.body;
    for(const type of ['keydown','keyup'])target.dispatchEvent(new KeyboardEvent(type,{key:'Escape',code:'Escape',keyCode:27,which:27,bubbles:true,cancelable:true}));
  }
  async function originalImage(media,owner){
    const thumbnail=sourceOf(media),base=sizeOf(media);
    const box=media.closest('[data-testid^="hatch-chat-attachment-presentation-"]');
    const button=media.closest('button,[role="button"]');
    if(!box||!button||!box.contains(button)||button.disabled||button.getAttribute('aria-disabled')==='true')throw new Error('original-unavailable');
    const before=new Map(Array.from(document.querySelectorAll('img')).map(element=>[element,{src:sourceOf(element),...sizeOf(element),visible:visible(element)}]));
    button.click();
    let last='',stable=0;
    const deadline=Date.now()+20000;
    while(Date.now()<deadline){
      await wait(250);
      if(job!==owner||owner.controller.signal.aborted)throw new Error('cancelled');
      const candidates=Array.from(document.querySelectorAll('img')).filter(element=>{
        const src=sourceOf(element),size=sizeOf(element),old=before.get(element);
        if(!src||src===thumbnail||element.complete===false||!visible(element)||element.closest('[data-testid^="hatch-chat-attachment-presentation-"]'))return false;
        if(size.width*size.height<=base.width*base.height*1.2||!base.height||!size.height)return false;
        if(Math.abs((size.width/size.height)/(base.width/base.height)-1)>0.04)return false;
        return !old||old.src!==src||old.width<size.width||old.height<size.height;
      }).sort((a,b)=>sizeOf(b).width*sizeOf(b).height-sizeOf(a).width*sizeOf(a).height);
      if(!candidates.length)continue;
      const modal=candidates.filter(element=>element.closest('[role="dialog"],[aria-modal="true"]'));
      const scoped=modal.length?modal:candidates;
      if(new Set(scoped.map(sourceOf)).size>1){last='';stable=0;continue;}
      const element=scoped[0],size=sizeOf(element),signature=`${sourceOf(element)}:${size.width}:${size.height}`;
      // A reused viewer node and a delayed original load are both valid; an old large page image is not.
      if(signature===last)stable++;else {last=signature;stable=0;}
      if(stable>=2)return {element,opened:true};
    }
    throw new Error('original-unavailable');
  }
  async function fetchMedia(media){
    if(job.fetching || job.media)return;
    job.fetching=true;const owner=job;
    try{
      let element=media;
      let opened=false;
      if(owner.mode==='image')({element,opened}=await originalImage(media,owner));
      const maximum=owner.mode==='video'?128*1024*1024:32*1024*1024;
      let downloaded;
      try{downloaded=await mediaBytes(sourceOf(element),owner,maximum);}
      finally{if(opened&&job===owner)closeViewer(element);}
      if(job!==owner)return;
      const {bytes,mime}=downloaded;
      if(!(owner.mode==='video'?mime.startsWith('video/'):mime.startsWith('image/')))throw new Error('wrong-media-type');
      owner.media={bytes,mime,original:owner.mode==='image',...sizeOf(element),attachment:attachmentId(media)};owner.state='media';
    }catch(error){if(job===owner){owner.state='error';owner.error=error.message==='media-too-large'?'生成文件超过本地大小限制':error.message==='original-unavailable'?'原图尚未加载成功，没有保存缩略图；请打开任务页面后重试':'生成完成，但取回媒体失败；可打开任务页面查看';}}
    finally{owner.fetching=false;}
  }
  async function recoverMedia(owner,thumbnail){
    owner.fetching=true;
    try{
      const expected=Uint8Array.from(atob(thumbnail),character=>character.charCodeAt(0));
      if(!expected.length||expected.length>2*1024*1024)throw new Error('invalid-thumbnail');
      const candidates=attachments().filter(box=>!box.closest('form,[class*="chat-user-bubble"],[class*="group/msg"]'))
        .map(box=>box.querySelector('img')).filter(element=>element&&element.naturalWidth&&sourceOf(element)).slice(-32);
      for(const element of candidates){
        if(job!==owner||owner.controller.signal.aborted)return;
        let bytes;try{({bytes}=await mediaBytes(sourceOf(element),owner,2*1024*1024));}
        catch(error){if(owner.controller.signal.aborted)throw error;continue;}
        if(bytes.length===expected.length&&bytes.every((value,index)=>value===expected[index])){
          owner.fetching=false;await fetchMedia(element);return;
        }
      }
      throw new Error('missing-original-task');
    }catch(error){if(job===owner){owner.state='error';owner.error='没有找到这张图片的原任务，请在该账号打开对应历史对话后重试；旧预览仍保留';}}
    finally{owner.fetching=false;}
  }
  window.__museStudio={
    recover(id,request){
      if(!trusted())return JSON.stringify({id,state:'error',error:'请先在 Muse 登录'});
      if(busy())return JSON.stringify({id,state:'error',error:'任务页面正在生成，请完成后再读取原图'});
      if(job&&!['done','error','cancelled'].includes(job.state))return result('error',{error:'这个账号正在处理另一个任务'});
      job={id,mode:'image',state:'recovering',recovering:true,controller:new AbortController(),fetching:false,media:null};
      void recoverMedia(job,request.thumbnail||'');
      return result('preparing');
    },
    begin(id,mode,prompt){
      if(!trusted())return result('error',{error:'请在 Muse 页面登录后重试'});
      if(job && !['done','error','cancelled'].includes(job.state))return result('error',{error:'这个账号正在处理另一个任务'});
      const ta=Array.from(document.querySelectorAll('textarea')).find(visible);
      if(!ta || ta.disabled)return result('waiting',{error:'正在等待账号聊天输入框就绪'});
      if(ta.value.trim() || busy())return result('error',{error:'任务页面已有未发送内容或正在生成，请先处理'});
      job={id,mode,prompt,state:'preparing',baseAgents:new Set(agents()),baseUsers:users().length,
        baseAttachments:new Set(attachments()),baseSources:new Set(attachments().flatMap(a=>Array.from(a.querySelectorAll('img,video')).map(m=>m.currentSrc||m.src))),
        controller:new AbortController(),last:'',stable:0,sentAt:0,accepted:false,fetching:false,media:null};
      const setter=Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype,'value').set;
      setter.call(ta,prompt);ta.dispatchEvent(new Event('input',{bubbles:true}));ta.dispatchEvent(new Event('change',{bubbles:true}));
      return result('preparing');
    },
    uploadChunk,
    commitUpload,
    attachedCount,
    poll(id){
      if(!trusted() || !job || job.id!==id)return JSON.stringify({id,state:'error',error:'任务页面已变化，请重新发送'});
      if(job.state==='error')return result('error',{error:job.error});
      if(job.state==='cancelled')return result('cancelled');
      if(job.media)return result('media',{text:job.recovering?'':job.last,mime:job.media.mime,bytes:job.media.bytes.length,
        original:job.media.original,width:job.media.width,height:job.media.height,attachment:job.media.attachment});
      if(job.recovering)return result('recovering');
      if(job.state==='preparing'){
        const ta=Array.from(document.querySelectorAll('textarea')).find(visible),button=ta && sendButton(ta);
        if(!button || button.disabled || button.getAttribute('aria-disabled')==='true')return result('preparing');
        job.state='sending';job.sentAt=Date.now();button.click();return result('sending');
      }
      scrollBottom();
      const newAgents=agents().filter(a=>!job.baseAgents.has(a));
      const text=newAgents.map(a=>(a.innerText||'').trim()).filter(Boolean).join('\n\n').slice(0,100000);
      if(users().length>job.baseUsers || newAgents.length || busy())job.accepted=true;
      if(!job.accepted && Date.now()-job.sentAt>25000){job.state='error';return result('error',{error:'网页未确认消息送达；请打开任务页面检查连接'});}
      if(job.mode!=='chat' && !job.fetching){
        const boxes=attachments().filter(a=>!job.baseAttachments.has(a));
        for(const box of boxes){
          if(box.closest('form,[class*="chat-user-bubble"],[class*="group/msg"]'))continue;
          const element=job.mode==='video'?box.querySelector('video'):box.querySelector('img');
          if(!element || (job.mode==='video'?element.readyState<2:!element.naturalWidth))continue;
          const src=element.currentSrc||element.src;
          if(!src || job.baseSources.has(src))continue;
          void fetchMedia(element);break;
        }
      }
      if(text===job.last && !busy())job.stable++;else job.stable=0;
      job.last=text;
      if(text && job.stable>=6 && job.accepted && !job.fetching){
        if(job.mode==='chat'){job.state='done';return result('done',{text});}
        if(Date.now()-job.sentAt>20000 && /无法|失败|不支持|不能|无法生成|cannot|can't|unable|unavailable|not supported/i.test(text)){
          job.state='error';return result('error',{text,error:'Muse 未返回所选类型的生成结果，请查看回复'});
        }
      }
      return result(job.fetching?'downloading':job.accepted?'running':'sending',{text});
    },
    read(id,offset){
      if(!trusted() || !job || job.id!==id || !job.media)return JSON.stringify({id,state:'error',error:'生成文件已不可用'});
      const bytes=job.media.bytes,chunk=bytes.subarray(offset,Math.min(offset+65536,bytes.length));
      let binary='';for(let i=0;i<chunk.length;i++)binary+=String.fromCharCode(chunk[i]);
      return result('chunk',{offset,data:btoa(binary),eof:offset+chunk.length>=bytes.length});
    },
    release(id){if(job?.id===id){job.controller.abort();job.media=null;job.state='done';}},
    cancel(id){if(job?.id===id){job.controller.abort();job.media=null;job.state='cancelled';if(!job.recovering)document.querySelector('[data-testid="hatch-composer-stop-button"]')?.click();}},
  };
})();
