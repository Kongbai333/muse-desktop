const assert=require('node:assert/strict'),vm=require('node:vm'),fs=require('node:fs');
const source=fs.readFileSync(require.resolve('../assets/muse-studio.js'),'utf8');

class Input {
  constructor(){this.files=null;this.events=[];this.accept='.jpg,.jpeg,.png';this.disabled=false;}
  getClientRects(){return [{}];}
  dispatchEvent(event){this.events.push(event.type);}
  closest(){return null;}
}
class Transfer {
  constructor(){this.files=[];this.items={add:file=>this.files.push(file)};}
}
class BrowserFile {
  constructor(parts,name,options){this.parts=parts;this.name=name;this.type=options.type;}
}

const input=new Input();
const composer={contains:node=>node===input,querySelectorAll:()=>[]};
const textarea={getClientRects(){return [{}];},closest(){return null;},parentElement:composer};
const document={body:{},querySelector:selector=>selector==='textarea'?textarea:null,querySelectorAll(selector){
  if(selector==='input[type="file"]')return [input];
  if(selector==='textarea')return [textarea];
  return [];
}};
const context={document,location:new URL('https://muse.ai/thread/new'),URL,Uint8Array,AbortController,setTimeout,Date,
  File:BrowserFile,DataTransfer:Transfer,Event:class{constructor(type){this.type=type;}},
  atob:value=>Buffer.from(value,'base64').toString('binary'),
  getComputedStyle:()=>({visibility:'visible'}),btoa:value=>Buffer.from(value,'binary').toString('base64')};
context.window=context;context.top=context;
vm.runInNewContext(source,context);
const api=context.__museStudio;
const bytes=Buffer.alloc(24*1024+7);for(let index=0;index<bytes.length;index++)bytes[index]=index%251;
let reply=JSON.parse(api.uploadChunk('ref',0,bytes.subarray(0,24*1024).toString('base64'),false));
assert.deepEqual([reply.state,reply.offset],['uploading',24*1024]);
assert.equal(JSON.parse(api.uploadChunk('ref',24*1024,bytes.subarray(24*1024).toString('base64'),true)).state,'uploaded');
reply=JSON.parse(api.commitUpload('ref','reference.jpg','image/jpeg'));
assert.equal(reply.state,'attached');
assert.equal(input.files.length,1);
assert.equal(input.files[0].name,'reference.jpg');
assert.equal(input.files[0].type,'image/jpeg');
assert.deepEqual(input.events,['input','change']);
assert.deepEqual(Buffer.concat(input.files[0].parts.map(part=>Buffer.from(part))),bytes);
assert.equal(JSON.parse(api.commitUpload('ref','reference.jpg','text/plain')).state,'error');
assert.equal(JSON.parse(api.uploadChunk('retry',0,'AQ==',false)).state,'uploading');
assert.equal(JSON.parse(api.uploadChunk('retry',2,'Ag==',true)).state,'error','out-of-order chunks must be rejected');
const deniedContext={...context,location:new URL('https://example.test/')};delete deniedContext.__museStudio;
deniedContext.window=deniedContext;deniedContext.top=deniedContext;vm.runInNewContext(source,deniedContext);
assert.equal(JSON.parse(deniedContext.__museStudio.uploadChunk('ref',0,'AQ==',true)).state,'error');
reply=JSON.parse(api.attachedCount('ref',1));
assert.equal(reply.count,0);assert.ok(reply.diag.includes('@输入区'),'an unconfirmed attachment must report diagnostics');
const pending={...context,document:{body:{},querySelector:()=>null,querySelectorAll:()=>[]}};delete pending.__museStudio;
pending.window=pending;pending.top=pending;vm.runInNewContext(source,pending);
pending.__museStudio.uploadChunk('wait',0,'AQ==',true);
assert.equal(JSON.parse(pending.__museStudio.commitUpload('wait','a.jpg','image/jpeg')).state,'locating',
  'a missing upload control must be retryable while the composer hydrates');
console.log('Reference image upload adapter contract passed.');
