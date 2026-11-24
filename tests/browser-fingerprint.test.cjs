const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const template = fs.readFileSync('crates/muse-desktop/src/browser_fingerprint.js', 'utf8');

// A deterministic raster fixture for the adapter's behavior. Native WebView coverage is separate.
function environment(key = [1, 2, 3, 4, 5, 6, 7, 8], origin = 'https://synthetic.example') {
  class Context {
    constructor(canvas, attributes) { this.canvas = canvas; this.attributes = attributes || {}; }
    getContextAttributes() { return { colorSpace: this.attributes.colorSpace || 'srgb' }; }
    drawImage(canvas) { this.canvas.pixels.set(canvas.pixels); this.canvas.tainted ||= canvas.tainted; }
    getImageData(x, y, w, h) {
      if (this.canvas.tainted) throw new DOMException('synthetic taint', 'SecurityError');
      if (!w || !h) throw new DOMException('zero dimension', 'IndexSizeError');
      const data = new Uint8ClampedArray(w * h * 4);
      for (let yy = 0; yy < h; yy++) for (let xx = 0; xx < w; xx++) {
        const sx = x + xx, sy = y + yy;
        if (sx >= 0 && sy >= 0 && sx < this.canvas.width && sy < this.canvas.height)
          data.set(this.canvas.pixels.slice((sy * this.canvas.width + sx) * 4, (sy * this.canvas.width + sx) * 4 + 4), (yy * w + xx) * 4);
      }
      return { data, width: w, height: h, colorSpace: this.getContextAttributes().colorSpace };
    }
    putImageData(image) { this.canvas.pixels.set(image.data); }
  }
  class Canvas {
    constructor() { this.width = 32; this.height = 32; this.pixels = new Uint8ClampedArray(32 * 32 * 4); this.context = null; }
    getContext(kind, options) { if (kind !== '2d') return null; return this.context ||= new Context(this, options); }
    toDataURL() { if (this.tainted) throw new DOMException('synthetic taint', 'SecurityError'); return Array.from(this.pixels).join(','); }
    toBlob(callback) { callback(this.toDataURL()); }
  }
  class GL {
    constructor(canvas) { this.canvas = canvas; this.RGBA = 0x1908; this.UNSIGNED_BYTE = 0x1401; this.FRAMEBUFFER = 0x8d40; this.FRAMEBUFFER_COMPLETE = 0x8cd5; this.PACK_ALIGNMENT = 0xd05; this.packing = 4; }
    getParameter(name) { return name === this.PACK_ALIGNMENT ? this.packing : 0; }
    checkFramebufferStatus() { return this.FRAMEBUFFER_COMPLETE; }
    isContextLost() { return false; }
    readPixels(x, y, w, h, format, type, data) {
      if (format !== this.RGBA || type !== this.UNSIGNED_BYTE || data.length < w * h * 4) return;
      for (let yy = 0; yy < h; yy++) for (let xx = 0; xx < w; xx++) {
        const sy = this.canvas.height - 1 - y - yy;
        data.set(this.canvas.pixels.slice((sy * this.canvas.width + x + xx) * 4, (sy * this.canvas.width + x + xx) * 4 + 4), (yy * w + xx) * 4);
      }
    }
  }
  const document = { createElement: () => new Canvas() };
  const scope = { HTMLCanvasElement: Canvas, CanvasRenderingContext2D: Context, WebGLRenderingContext: GL,
    document, origin, location: { origin }, Uint8ClampedArray, Uint8Array, ArrayBuffer, DOMException, console };
  const rawRead = Context.prototype.getImageData;
  const rawExport = Canvas.prototype.toDataURL;
  vm.runInNewContext(template.replace('__MUSE_FINGERPRINT_SEED__', JSON.stringify(key)), scope);
  function painted() { const c = new Canvas(); for (let i = 0; i < c.pixels.length; i += 4) c.pixels.set([100, 120, 140, 255], i); c.getContext('2d'); return c; }
  return { Canvas, Context, GL, scope, painted, rawRead, rawExport };
}
const read = c => c.getContext('2d').getImageData(0, 0, c.width, c.height).data;

test('painted readouts are stable, account-specific and site-separated without changing the raster', () => {
  const a = environment(), b = environment([9, 10, 11, 12, 13, 14, 15, 16]), site = environment(undefined, 'https://other.example');
  const c = a.painted(), baseline = Array.from(c.pixels), first = Array.from(read(c));
  assert.notDeepEqual(first, baseline);
  assert.deepEqual(Array.from(read(c)), first);
  assert.deepEqual(Array.from(read(a.painted())), first);
  assert.deepEqual(Array.from(c.pixels), baseline);
  assert.notDeepEqual(Array.from(read(b.painted())), first);
  assert.notDeepEqual(Array.from(read(site.painted())), first);
});
test('subrectangles agree with full readout at the same absolute coordinates', () => {
  const e = environment(), c = e.painted(), full = read(c), part = c.getContext('2d').getImageData(3, 4, 10, 9).data;
  for (let y = 0; y < 9; y++) for (let x = 0; x < 10; x++)
    assert.deepEqual(Array.from(part.slice((y * 10 + x) * 4, (y * 10 + x + 1) * 4)), Array.from(full.slice(((y + 4) * 32 + x + 3) * 4, ((y + 4) * 32 + x + 4) * 4)));
});
test('data URL and blob encode exactly the adapted pixels once', () => {
  const e = environment(), c = e.painted(), expected = Array.from(read(c)).join(',');
  assert.equal(c.toDataURL(), expected);
  c.toBlob(blob => assert.equal(blob, expected));
  assert.equal(c.toDataURL(), expected);
  assert.notEqual(e.rawExport.call(c), expected);
});
test('blank, alpha, tainted and invalid native reads keep their semantics', () => {
  const e = environment(), c = new e.Canvas();
  assert.deepEqual(Array.from(read(c)), Array.from(c.pixels));
  const p = e.painted(), output = read(p);
  for (let i = 3; i < output.length; i += 4) assert.equal(output[i], 255);
  p.tainted = true;
  assert.throws(() => read(p), { name: 'SecurityError' });
  assert.throws(() => p.toDataURL(), { name: 'SecurityError' });
  assert.throws(() => c.getContext('2d').getImageData(0, 0, 0, 1), { name: 'IndexSizeError' });
});
test('WebGL RGBA8 readback uses matching top-left canvas coordinates and keeps alpha', () => {
  const e = environment(), c = e.painted(), gl = new e.GL(c), output = new Uint8Array(c.pixels.length), canvas = read(c);
  gl.readPixels(0, 0, 32, 32, gl.RGBA, gl.UNSIGNED_BYTE, output);
  assert.notDeepEqual(Array.from(output), Array.from(c.pixels));
  for (let y = 0; y < 32; y++)
    assert.deepEqual(Array.from(output.slice(y * 128, (y + 1) * 128)), Array.from(canvas.slice((31 - y) * 128, (32 - y) * 128)));
});
test('unsupported pixel types and display-p3 do not get silently re-encoded as srgb', () => {
  const e = environment(), c = new e.Canvas(); c.getContext('2d', { colorSpace: 'display-p3' });
  c.pixels.fill(255);
  assert.equal(c.toDataURL(), e.rawExport.call(c));
  const gl = new e.GL(e.painted()), data = new Uint8Array(32 * 32 * 4).fill(17);
  gl.readPixels(0, 0, 32, 32, 0, gl.UNSIGNED_BYTE, data);
  assert(data.every(x => x === 17));
});
