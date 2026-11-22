// Extracted from the MUSE2API admin form (MIT).
// Copyright (c) 2026 MUSE2API Authors. See THIRD_PARTY_NOTICES.md.
async function addAccount(fromManual){
  const labelEl = $(fromManual ? 'mLabel' : 'newLabel');
  const ckEl    = $(fromManual ? 'mCookie' : 'newCookie');
  const label = labelEl.value.trim();
  const raw = ckEl.value.trim();
  if (!raw) return toast('请先粘贴 Cookie', 'warn');
  const lines = raw.split('\n').map(s => s.trim()).filter(Boolean);
  const body = (lines.length > 1) ? {batch: raw} : {label: label, cookie_header: raw};
  try{
    const r = await api('/admin/accounts', {method:'POST', body: JSON.stringify(body)});
    if (r.warning) toast(r.warning, 'warn');
    else toast(`成功导入 ${r.count} 个账号`, 'ok');
    ckEl.value = ''; labelEl.value = '';
    loadPool();
  }catch(e){ toast('导入失败：' + e.message, 'bad'); }
}

