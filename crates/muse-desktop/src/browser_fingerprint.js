// Account-stable, origin-separated readback adaptation. No native identity spoofing.
(() => {
  'use strict';
  const key = __MUSE_FINGERPRINT_SEED__;
  const apply = Reflect.apply, describe = Object.getOwnPropertyDescriptor;
  const tag = Object.prototype.toString, maxPixels = 4 * 1024 * 1024;
  const contexts = new WeakMap();
  let site = 2166136261;
  for (const c of String(globalThis.origin || globalThis.location?.origin || 'null'))
    site = Math.imul(site ^ c.charCodeAt(0), 16777619) >>> 0;
  const mix = x => {
    x = Math.imul(x ^ (x >>> 16), 0x7feb352d);
    x = Math.imul(x ^ (x >>> 15), 0x846ca68b);
    return (x ^ (x >>> 16)) >>> 0;
  };
  const bytes = value => ArrayBuffer.isView(value) &&
    ['[object Uint8ClampedArray]', '[object Uint8Array]'].includes(apply(tag, value, []));
  function bounded(canvas) {
    return canvas && Number.isSafeInteger(canvas.width) && Number.isSafeInteger(canvas.height) &&
      canvas.width > 0 && canvas.height > 0 && canvas.width * canvas.height <= maxPixels;
  }
  function adapt(data, width, height, x, y, flipped, canvasHeight, offset = 0) {
    if (!bytes(data) || width * height > maxPixels || data.length - offset < width * height * 4) return;
    for (let row = 0; row < height; row++) {
      const yy = flipped ? canvasHeight - 1 - y - row : y + row;
      for (let col = 0; col < width; col++) {
        const i = offset + (row * width + col) * 4, xx = x + col;
        // Translucent premultiplication can amplify a 1-bit change; leave it intact.
        if (data[i + 3] !== 255) continue;
        const h = mix(key[((xx & 3) | ((yy & 1) << 2))] ^ site ^
          Math.imul(xx, 73856093) ^ Math.imul(yy, 19349663));
        if ((h & 15) === 0) data[i + ((h >>> 8) % 3)] ^= 1;
      }
    }
  }
  function install(proto, name, build) {
    if (!proto) return;
    const descriptor = describe(proto, name);
    if (!descriptor || typeof descriptor.value !== 'function') return;
    try { Object.defineProperty(proto, name, { ...descriptor, value: build(descriptor.value) }); } catch (_) {}
  }
  const htmlProto = globalThis.HTMLCanvasElement?.prototype;
  const offscreenProto = globalThis.OffscreenCanvas?.prototype;
  const canvasTypes = [...new Set([htmlProto, offscreenProto].filter(Boolean))];
  const read2D = new Map();
  for (const proto of new Set([globalThis.CanvasRenderingContext2D?.prototype,
    globalThis.OffscreenCanvasRenderingContext2D?.prototype].filter(Boolean))) {
    const nativeRead = proto.getImageData;
    read2D.set(proto, nativeRead);
    install(proto, 'getImageData', original => function getImageData(...args) {
      const result = apply(original, this, args);
      if (!bounded(this.canvas) || result.colorSpace === 'display-p3' ||
        !args.slice(0, 4).every(v => typeof v === 'number' && Number.isFinite(v) && Math.abs(v) < 0x80000000)) return result;
      let [x, y, w, h] = args.slice(0, 4).map(Math.trunc);
      if (w < 0) x += w;
      if (h < 0) y += h;
      adapt(result.data, result.width, result.height, x, y, false, this.canvas.height);
      return result;
    });
  }
  for (const proto of canvasTypes) {
    const nativeContext = proto.getContext;
    install(proto, 'getContext', original => function getContext(...args) {
      const context = apply(original, this, args);
      if (context) {
        const space = context.getContextAttributes?.()?.colorSpace ?? context.drawingBufferColorSpace;
        contexts.set(this, space === undefined || space === 'srgb');
      }
      return context;
    });
    function copy(source) {
      if (!bounded(source) || contexts.get(source) !== true) return source;
      const canvas = proto === htmlProto ? document.createElement('canvas') : new OffscreenCanvas(source.width, source.height);
      canvas.width = source.width; canvas.height = source.height;
      const context = apply(nativeContext, canvas, ['2d']);
      if (!context) return source;
      context.drawImage(source, 0, 0);
      const entry = [...read2D].find(([p]) => p.isPrototypeOf(context));
      if (!entry) return source;
      // Use captured native reads so PNG export is perturbed exactly once.
      const image = apply(entry[1], context, [0, 0, canvas.width, canvas.height]);
      adapt(image.data, image.width, image.height, 0, 0, false, canvas.height);
      context.putImageData(image, 0, 0);
      return canvas;
    }
    install(proto, 'toDataURL', original => function toDataURL(...args) {
      return apply(original, args[0] === undefined || args[0] === 'image/png' ? copy(this) : this, args);
    });
    install(proto, 'toBlob', original => function toBlob(...args) {
      const eligible = typeof args[0] === 'function' && (args[1] === undefined || args[1] === 'image/png');
      return apply(original, eligible ? copy(this) : this, args);
    });
    install(proto, 'convertToBlob', original => function convertToBlob(...args) {
      // Preserve options getters and the native Promise rejection contract.
      if (args.length && args[0] !== undefined) return apply(original, this, args);
      try { return apply(original, copy(this), args); } catch (error) { return Promise.reject(error); }
    });
  }
  for (const proto of new Set([globalThis.WebGLRenderingContext?.prototype,
    globalThis.WebGL2RenderingContext?.prototype].filter(Boolean))) {
    const parameter = proto.getParameter, status = proto.checkFramebufferStatus;
    install(proto, 'readPixels', original => function readPixels(...args) {
      const result = apply(original, this, args);
      const [x, y, w, h, format, type, data, extra = 0] = args;
      if (!bounded(this.canvas) || format !== 0x1908 || type !== 0x1401 || !bytes(data) ||
        ![x, y, w, h, extra].every(Number.isSafeInteger) || w <= 0 || h <= 0 || extra < 0 ||
        x < 0 || y < 0 || x + w > this.canvas.width || y + h > this.canvas.height ||
        (this.drawingBufferColorSpace && this.drawingBufferColorSpace !== 'srgb') ||
        (this.isContextLost && this.isContextLost()) ||
        apply(status, this, [this.READ_FRAMEBUFFER || 0x8d40]) !== 0x8cd5) return result;
      if ((w * 4) % apply(parameter, this, [this.PACK_ALIGNMENT || 0xd05]) !== 0) return result;
      // PBO/non-default WebGL2 packing is deliberately left native.
      if (this.PIXEL_PACK_BUFFER_BINDING !== undefined &&
        (apply(parameter, this, [this.PIXEL_PACK_BUFFER_BINDING]) !== null ||
          [this.PACK_ROW_LENGTH, this.PACK_SKIP_ROWS, this.PACK_SKIP_PIXELS].some(p => apply(parameter, this, [p]) !== 0) ||
          apply(parameter, this, [this.READ_BUFFER]) === this.NONE)) return result;
      adapt(data, w, h, x, y, true, this.canvas.height, extra);
      return result;
    });
  }
})();
