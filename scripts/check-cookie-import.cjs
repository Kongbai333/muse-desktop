// Runs the real reference admin form handler offline using synthetic Cookie data.
const assert=require('node:assert/strict'),fs=require('node:fs'),vm=require('node:vm');
const source=fs.readFileSync(require.resolve('../tests/fixtures/muse2api-add-account.js'),'utf8');
const fixtures=JSON.parse(fs.readFileSync(process.argv[2],'utf8'));
const parseHeader=text=>Object.fromEntries(text.split(';').map(part=>part.trim()).filter(part=>part.includes('='))
  .map(part=>{const at=part.indexOf('=');return[part.slice(0,at).trim(),part.slice(at+1).trim()];}));
(async()=>{
  for(const [kind,raw] of Object.entries(fixtures)){
    let request;
    const fields={newLabel:{value:''},newCookie:{value:raw}};
    const context={$:id=>fields[id],api:async(path,options)=>{request={path,body:JSON.parse(options.body)};return{count:kind==='single'?1:2};},toast(){},loadPool(){}};
    vm.runInNewContext(source,context);
    await context.addAccount(false);
    assert.equal(request.path,'/admin/accounts');
    if(kind==='single'){
      assert.equal(typeof request.body.cookie_header,'string');assert.equal(request.body.batch,undefined);
      assert.equal(parseHeader(request.body.cookie_header).hatch_sess,'fixture-single==');
    }else{
      assert.equal(typeof request.body.batch,'string');assert.equal(request.body.cookie_header,undefined);
      const rows=request.body.batch.split('\n').map(line=>{const at=line.indexOf('|');return{label:line.slice(0,at).trim(),cookies:parseHeader(line.slice(at+1))};});
      assert.equal(rows.length,2);assert.equal(rows[0].cookies.hatch_sess,'fixture-first==');assert.equal(rows[1].cookies.hatch_sess,'fixture-second');
      assert.equal(rows[0].label,'主号 test extra');assert.equal(rows[1].label,'小号');
    }
  }
  console.log('Reference admin import handler accepted the single-header and labelled-batch fixtures.');
})().catch(error=>{console.error(error);process.exitCode=1;});
