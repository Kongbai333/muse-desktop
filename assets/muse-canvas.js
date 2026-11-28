
    /**
     * Muse Canvas · 专业级 AI 视觉工作流无限画布
     * 100% 原生 Vanilla JS + 内联 CSS，零外部依赖
     */
    (function () {
      'use strict';

      /* ========================================================================
         1. 常量与预设配置
         ======================================================================== */
      const DEFAULT_ACCOUNTS = [{ id: "", name: "请选择桌面账号", responseFormat: "url" }];

      // Reuse the desktop icon geometry for existing actions; playback icons
      // share its 24px grid and 1.65px rounded stroke, never font/emoji glyphs.
      const UI_ICONS = {
        play: '<path d="M7 4v16l13 -8z" fill="currentColor"/>',
        pause: '<path d="M9 5v14M15 5v14"/>',
        volume: '<path d="M11 5L6 9H3v6h3l5 4zM15 8a6 6 0 0 1 0 8M18 5a10 10 0 0 1 0 14"/>',
        muted: '<path d="M11 5L6 9H3v6h3l5 4zM16 9l5 6M21 9l-5 6"/>',
        expand: '<path d="M16 4h4v4M14 10l6 -6M8 20H4v-4M4 20l6 -6"/>',
        prompt: '<path d="M9 5H7a2 2 0 0 0 -2 2v12a2 2 0 0 0 2 2h10a2 2 0 0 0 2 -2V7a2 2 0 0 0 -2 -2h-2M9 12h6M9 16h4"/><path d="M9 5a2 2 0 0 1 2 -2h2a2 2 0 0 1 2 2a2 2 0 0 1 -2 2h-2a2 2 0 0 1 -2 -2z"/>',
        settings: '<path d="M10.325 4.317c.426 -1.756 2.924 -1.756 3.35 0a1.724 1.724 0 0 0 2.573 1.066c1.543 -.94 3.31 .826 2.37 2.37a1.724 1.724 0 0 0 1.065 2.572c1.756 .426 1.756 2.924 0 3.35a1.724 1.724 0 0 0 -1.066 2.573c.94 1.543 -.826 3.31 -2.37 2.37a1.724 1.724 0 0 0 -2.572 1.065c-.426 1.756 -2.924 1.756 -3.35 0a1.724 1.724 0 0 0 -2.573 -1.066c-1.543 .94 -3.31 -.826 -2.37 -2.37a1.724 1.724 0 0 0 -1.065 -2.572c-1.756 -.426 -1.756 -2.924 0 -3.35a1.724 1.724 0 0 0 1.066 -2.573c-.94 -1.543 .826 -3.31 2.37 -2.37c1 .608 2.296 .07 2.572 -1.065"/><path d="M9 12a3 3 0 1 0 6 0a3 3 0 0 0 -6 0"/>',
      };
      const uiIcon = name => `<svg class="canvas-ui-icon" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false">${UI_ICONS[name] || ''}</svg>`;
      const snapPixel = value => Math.round(value * (window.devicePixelRatio || 1)) / (window.devicePixelRatio || 1);

      const IMAGE_RATIOS = [
        { value: '1:1',  w: 300, h: 300, renderW: 768, renderH: 768 },
        { value: '16:9', w: 368, h: 207, renderW: 960, renderH: 540 },
        { value: '9:16', w: 230, h: 408, renderW: 540, renderH: 960 },
        { value: '4:3',  w: 320, h: 240, renderW: 800, renderH: 600 },
        { value: '3:4',  w: 255, h: 340, renderW: 600, renderH: 800 },
      ];

      const VIDEO_RATIOS = [
        { value: '16:9', label: '16:9 横屏', w: 368, h: 207 },
        { value: '9:16', label: '9:16 竖屏', w: 230, h: 408 },
      ];

      const VIDEO_DURATIONS = [5, 6, 8, 10];

      const PROMPT_PRESETS = [
        { name: '索尼 35mm 夜景', text: '索尼 A7S3，35mm f/1.4 电影定焦，雨后霓虹街道低照度写实摄影，冷蓝与琥珀暖光强烈对比，湿润路面倒影，细腻胶片颗粒' },
        { name: '微缩等距 3D 场景', text: '等距视角微缩建筑模型，极简包豪斯玻璃温室工作室，柔和体积晨光，移轴景深，精细哑光材质，C4D Octane 级写实渲染' },
        { name: '赛博霓虹雨夜', text: '未来东京高架桥雨夜，全息广告牌穿透薄雾，赛博朋克电影宽银幕构图，青橙色调分级，镜头眩光与雨水折射细节' },
        { name: '电影级推镜头', text: '极缓慢平滑推镜（Slow Push-In），焦点从前景雨滴平滑过渡至中景建筑光影，空气中漂浮微尘粒子，24fps 电影动态模糊' },
      ];

      const CAMERA_TAGS = [
        '35mm f/1.4 低光',
        '85mm 特写',
        '缓慢推镜',
        '环绕航拍',
        '固定机位',
        '移轴微缩',
      ];

      /* ========================================================================
         2. 全局应用状态与撤销/重做栈
         ======================================================================== */
      const state = {
        mode: 'desktop',
        viewport: { x: 0, y: 0, scale: 1 },
        minScale: 0.25,
        maxScale: 2.5,
        gridSize: 40,
        accounts: [],
        nodes: [],
        connections: [],
        selectedIds: new Set(),
        hoveredEdgeId: null,
        nextZ: 10,
        uid: 1,
        isSpaceDown: false,
        history: [],
        historyIndex: -1,
      };

      const interact = {
        mode: null, // 'pan' | 'drag-nodes' | 'connect' | 'marquee' | 'minimap'
        startX: 0,
        startY: 0,
        viewX: 0,
        viewY: 0,
        dragOrigins: new Map(),
        movedDuringDrag: false,
        connectFromNodeId: null,
        connectFromPort: null,
        snapTarget: null,
        pendingQuickWire: null,
        fileTargetNodeId: null,
        fileTargetMode: 'ref', // 'ref' | 'replace-image' | 'replace-video'
        dragDepth: 0,
      };

      const activeTasks = new Map();


      /* ========================================================================
         3. DOM 引用
         ======================================================================== */
      const canvasEl = document.getElementById('canvas-container');
      const viewportEl = document.getElementById('viewport');
      const sceneEl = document.getElementById('canvas-scene');
      const edgesGroup = document.getElementById('edges-group');
      const tempEdge = document.getElementById('temp-edge');
      const edgeBadgesLayer = document.getElementById('edge-badges-layer');
      const nodesLayer = document.getElementById('nodes-layer');
      const marqueeBox = document.getElementById('marquee-box');

      const topActionPill = document.getElementById('top-action-pill');
      const nodeInfoPopover = document.getElementById('node-info-popover');
      const genPanel = document.getElementById('gen-floating-panel');
      const panelRefStrip = document.getElementById('panel-ref-strip');
      const panelUpstreamPills = document.getElementById('panel-upstream-pills');
      const panelPromptInput = document.getElementById('panel-prompt-input');
      const panelErrorBanner = document.getElementById('panel-error-banner');
      const panelEndpointHint = document.getElementById('panel-endpoint-hint');
      const panelAccountLabel = document.getElementById('panel-account-label');
      const panelParamsLabel = document.getElementById('panel-params-label');
      const panelCameraLabel = document.getElementById('panel-camera-label');
      const btnPanelCamera = document.getElementById('btn-panel-camera');
      const btnPanelGenerate = document.getElementById('btn-panel-generate');

      const inlinePopover = document.getElementById('inline-popover');
      const quickCreateMenu = document.getElementById('quick-create-menu');
      const zoomResetBtn = document.getElementById('btn-zoom-reset');
      const btnUndo = document.getElementById('btn-undo');
      const btnRedo = document.getElementById('btn-redo');

      const minimapWrap = document.getElementById('minimap-wrap');
      const minimapCanvas = document.getElementById('minimap-canvas');
      const minimapCtx = minimapCanvas.getContext('2d');

      const accountsModal = document.getElementById('accounts-modal');
      const accountsListEl = document.getElementById('accounts-list');

      const lightboxModal = document.getElementById('lightbox-modal');
      const lightboxContent = document.getElementById('lightbox-content');
      const lightboxDownload = document.getElementById('lightbox-download');
      const toastContainer = document.getElementById('toast-container');
      const dropOverlay = document.getElementById('drop-overlay');

      const fileInputImage = document.getElementById('file-input-image');
      const fileInputVideo = document.getElementById('file-input-video');
      const fileInputJson = document.getElementById('file-input-json');

      /* ========================================================================
         4. 工具函数与账号持久化
         ======================================================================== */
      const genId = (prefix = 'n') => `${prefix}_${Date.now().toString(36)}_${state.uid++}`;
      const clamp = (v, min, max) => Math.min(Math.max(v, min), max);
      const sleep = (ms) => new Promise(r => setTimeout(r, ms));
      const esc = (s) => String(s || '').replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

      function getAccount(id) {
        return state.accounts.find(a => a.id === id) || DEFAULT_ACCOUNTS[0];
      }

      function screenToWorld(cx, cy) {
        return {
          x: (cx - state.viewport.x) / state.viewport.scale,
          y: (cy - state.viewport.y) / state.viewport.scale,
        };
      }

      function worldToScreen(wx, wy) {
        return {
          x: wx * state.viewport.scale + state.viewport.x,
          y: wy * state.viewport.scale + state.viewport.y,
        };
      }

      function isInputFocused() {
        const el = document.activeElement;
        return el && (['INPUT', 'TEXTAREA', 'SELECT'].includes(el.tagName) || el.isContentEditable);
      }

      function copyTextReliable(text) {
        const ta = document.createElement('textarea');
        ta.value = text;
        ta.style.position = 'fixed';
        ta.style.left = '-9999px';
        document.body.appendChild(ta);
        ta.select();
        try {
          document.execCommand('copy');
          showToast('已复制至剪贴板');
        } catch (_) {
          showToast('复制失败', 'error');
        }
        ta.remove();
      }

      function showToast(msg, type = 'info') {
        const el = document.createElement('div');
        el.className = `toast-item ${type === 'error' ? 'is-error' : ''}`;
        el.textContent = msg;
        toastContainer.appendChild(el);
        requestAnimationFrame(() => el.classList.add('is-visible'));
        setTimeout(() => {
          el.classList.remove('is-visible');
          setTimeout(() => el.remove(), 200);
        }, 2200);
      }

      function getNodeDimensions(node) {
        if (node.type === 'text') {
          const dom = document.getElementById(node.id);
          return { w: node.width || 272, h: dom ? dom.offsetHeight : 168 };
        }
        if (node.type === 'image') {
          const cfg = IMAGE_RATIOS.find(r => r.value === node.aspectRatio) || IMAGE_RATIOS[1];
          return { w: cfg.w, h: cfg.h };
        }
        const vcfg = VIDEO_RATIOS.find(r => r.value === node.aspectRatio) || VIDEO_RATIOS[0];
        return { w: vcfg.w, h: vcfg.h };
      }

      /* ========================================================================
         5. 历史记录（撤销 / 重做）
         ======================================================================== */
      function serializeGraph() {
        return JSON.stringify({
          viewport: { ...state.viewport },
          nodes: state.nodes.map(n => ({
            ...n,
            isGenerating: false,
            error: '',
            localRefs: (n.localRefs || []).map(r => ({ id: r.id, name: r.name, url: r.url })),
          })),
          connections: state.connections.map(c => ({ ...c })),
        });
      }

      function pushHistory() {
        const snap = historySnapshot();
        if (state.history[state.historyIndex] === snap) return;
        state.history = state.history.slice(0, state.historyIndex + 1);
        state.history.push(snap);
        if (state.history.length > 50) state.history.shift();
        state.historyIndex = state.history.length - 1;
        updateHistoryButtons();
        scheduleSave();
      }

      function historySnapshot() {
        // Viewport motion, selection stacking and job progress are not edits.
        const graph=validateGraph(JSON.parse(serializeGraph()));
        for(const node of graph.nodes) {
          delete node.z;delete node.elapsed;delete node.genStage;delete node.status;delete node.taskId;delete node.requestId;
        }
        return JSON.stringify(graph);
      }

      function restoreSnapshot(snapStr) {
        try {
          const data = JSON.parse(snapStr);
          const graph = validateGraph(data);
          [...activeTasks.values()].filter(task => task.canvasId === library.active).forEach(task => cancelTask(task.nodeId));

          state.nodes = graph.nodes;
          state.connections = graph.connections;
          state.nextZ = Math.max(10, ...state.nodes.map(n => n.z || 10));
          state.selectedIds.clear();
          nodesLayer.innerHTML = '';
          state.nodes.forEach(n => renderNode(n));
          renderEdges();
          updateFloatingUI();
          drawMinimap();
          return true;
        } catch (err) {
          showToast('工作流格式不受支持，未替换当前画布', 'error');
          return false;
        }
      }

      function undo() {
        if (state.historyIndex <= 0) return;
        state.historyIndex--;
        restoreSnapshot(state.history[state.historyIndex]);
        updateHistoryButtons();
        scheduleSave();
        showToast('已撤销');
      }

      function redo() {
        if (state.historyIndex >= state.history.length - 1) return;
        state.historyIndex++;
        restoreSnapshot(state.history[state.historyIndex]);
        updateHistoryButtons();
        scheduleSave();
        showToast('已重做');
      }

      function updateHistoryButtons() {
        btnUndo.disabled = state.historyIndex <= 0;
        btnRedo.disabled = state.historyIndex >= state.history.length - 1;
      }

      /* ========================================================================
         6. 视口平移、缩放与适应视图
         ======================================================================== */
      function applyViewport() {
        state.viewport.x = snapPixel(state.viewport.x);
        state.viewport.y = snapPixel(state.viewport.y);
        const { x, y, scale } = state.viewport;
        viewportEl.style.left = `${x}px`;
        viewportEl.style.top = `${y}px`;
        sceneEl.style.zoom = scale;
        const g = state.gridSize * scale;
        canvasEl.style.backgroundSize = `${g}px ${g}px`;
        canvasEl.style.backgroundPosition = `${x}px ${y}px`;
        zoomResetBtn.textContent = `${Math.round(scale * 100)}%`;
        updateFloatingUI();
        drawMinimap();
        scheduleSave();
      }

      function zoomAt(nextScale, cx, cy) {
        const s = clamp(nextScale, state.minScale, state.maxScale);
        if (Math.abs(s - state.viewport.scale) < 0.0001) return;
        const w = screenToWorld(cx, cy);
        state.viewport.scale = s;
        state.viewport.x = cx - w.x * s;
        state.viewport.y = cy - w.y * s;
        applyViewport();
      }

      function fitToView(silent = false, focusNodeId = null) {
        if (state.nodes.length === 0) {
          state.viewport = { x: window.innerWidth / 2, y: window.innerHeight / 2, scale: 1 };
          applyViewport();
          return;
        }
        let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
        state.nodes.forEach(n => {
          const d = getNodeDimensions(n);
          minX = Math.min(minX, n.x);
          minY = Math.min(minY, n.y - 30);
          maxX = Math.max(maxX, n.x + d.w);
          maxY = Math.max(maxY, n.y + d.h + 180);
        });
        const padX = window.innerWidth < 720 ? 36 : 80;
        const padY = 80;
        const cw = Math.max(maxX - minX, 320);
        const ch = Math.max(maxY - minY, 260);
        const aw = Math.max(window.innerWidth - padX * 2, 240);
        const ah = Math.max(window.innerHeight - padY * 2, 240);
        const s = clamp(Math.min(aw / cw, ah / ch, 1), state.minScale, 1);
        state.viewport.scale = s;

        if (focusNodeId) {
          const fn = state.nodes.find(x => x.id === focusNodeId);
          if (fn) {
            const fd = getNodeDimensions(fn);
            state.viewport.x = window.innerWidth / 2 - (fn.x + fd.w / 2) * s;
            state.viewport.y = window.innerHeight * 0.36 - (fn.y + fd.h / 2) * s;
            applyViewport();
            return;
          }
        }
        state.viewport.x = window.innerWidth / 2 - ((minX + maxX) / 2) * s;
        state.viewport.y = window.innerHeight / 2 - ((minY + maxY) / 2) * s;
        applyViewport();
        if (!silent) showToast('已适应全部节点');
      }

      /* ========================================================================
         7. 上下游数据聚合与语义角色计算
         ======================================================================== */
      function getUpstream(nodeId) {
        const texts = [];
        const images = [];
        state.connections.filter(c => c.to === nodeId).forEach(c => {
          const src = state.nodes.find(n => n.id === c.from);
          if (!src) return;
          if (src.type === 'text') {
            texts.push({ edgeId: c.id, nodeId: src.id, title: src.title || '文本', text: (src.content || '').trim() });
          } else if (src.type === 'image') {
            images.push({ edgeId: c.id, nodeId: src.id, source: 'upstream', title: src.title || '图片', url: src.outputUrl || '' });
          }
        });
        return { texts, images };
      }

      function getAllRefs(node) {
        const { images } = getUpstream(node.id);
        const validUpstream = images.filter(i => i.url);
        const locals = (node.localRefs || []).map(r => ({ ...r, source: 'local' }));
        return [...validUpstream, ...locals];
      }

      function getEffectivePrompt(node) {
        const { texts } = getUpstream(node.id);
        const parts = [];
        texts.forEach(t => { if (t.text) parts.push(t.text); });
        if ((node.prompt || '').trim()) parts.push(node.prompt.trim());
        if (Array.isArray(node.cameraTags) && node.cameraTags.length) {
          parts.push(`镜头参数：${node.cameraTags.join('，')}`);
        }
        return parts.join('，');
      }

      function getEdgeRoleLabel(conn) {
        const src = state.nodes.find(n => n.id === conn.from);
        const dst = state.nodes.find(n => n.id === conn.to);
        if (!src || !dst) return '连接';
        if (src.type === 'text') return '提示词';
        if (src.type === 'image') {
          const imgEdges = state.connections.filter(c => {
            const s = state.nodes.find(n => n.id === c.from);
            return c.to === conn.to && s && s.type === 'image';
          });
          const idx = imgEdges.findIndex(c => c.id === conn.id);
          if (dst.type === 'video' && idx === 0) return '首帧';
          return `参考图 #${idx + 1}`;
        }
        return '媒体流';
      }

      /* ========================================================================
         8. 桌面媒体由真实账号生成并保存在本机
         ======================================================================== */
      /* ========================================================================
         9. SVG 连线与中点交互徽章渲染
         ======================================================================== */
      function getPortWorldPos(node, side) {
        const d = getNodeDimensions(node);
        return {
          x: side === 'out' ? node.x + d.w : node.x,
          y: node.y + d.h / 2,
        };
      }

      function computeBezier(x1, y1, x2, y2) {
        const dx = Math.max(Math.abs(x2 - x1) * 0.48, 72);
        const cx1 = x1 + dx, cy1 = y1;
        const cx2 = x2 - dx, cy2 = y2;
        const path = `M ${x1} ${y1} C ${cx1} ${cy1}, ${cx2} ${cy2}, ${x2} ${y2}`;
        // 三次贝塞尔曲线 t=0.5 精确中点
        const midX = 0.125 * x1 + 0.375 * cx1 + 0.375 * cx2 + 0.125 * x2;
        const midY = 0.125 * y1 + 0.375 * cy1 + 0.375 * cy2 + 0.125 * y2;
        return { path, midX, midY };
      }

      function renderEdges() {
        edgesGroup.innerHTML = '';
        edgeBadgesLayer.innerHTML = '';

        // 标记已连接的端口
        document.querySelectorAll('.node-port').forEach(p => p.classList.remove('port-connected'));

        state.connections.forEach(conn => {
          const src = state.nodes.find(n => n.id === conn.from);
          const dst = state.nodes.find(n => n.id === conn.to);
          if (!src || !dst) return;

          document.getElementById(src.id)?.querySelector('.port-out')?.classList.add('port-connected');
          document.getElementById(dst.id)?.querySelector('.port-in')?.classList.add('port-connected');

          const p1 = getPortWorldPos(src, 'out');
          const p2 = getPortWorldPos(dst, 'in');
          const { path, midX, midY } = computeBezier(p1.x, p1.y, p2.x, p2.y);

          const isImageEdge = src.type === 'image';
          const isHovered = state.hoveredEdgeId === conn.id;
          const isGenerating = Boolean(dst.isGenerating);
          const roleLabel = getEdgeRoleLabel(conn);

          // SVG 连线组
          const g = document.createElementNS('http://www.w3.org/2000/svg', 'g');
          g.setAttribute('class', `edge-group ${isHovered ? 'is-hovered' : ''}`);
          g.dataset.edgeId = conn.id;

          const hit = document.createElementNS('http://www.w3.org/2000/svg', 'path');
          hit.setAttribute('d', path);
          hit.setAttribute('class', 'edge-hit');

          const vis = document.createElementNS('http://www.w3.org/2000/svg', 'path');
          vis.setAttribute('d', path);
          vis.setAttribute('class', `edge-path ${isImageEdge ? 'edge-type-image' : 'edge-type-text'}`);

          g.appendChild(hit);
          g.appendChild(vis);

          // 下游生成中：流动光脉冲粒子动效
          if (isGenerating) {
            const flow = document.createElementNS('http://www.w3.org/2000/svg', 'path');
            flow.setAttribute('d', path);
            flow.setAttribute('class', `edge-flow-overlay ${isImageEdge ? 'edge-flow-image' : 'edge-flow-text'}`);
            g.appendChild(flow);

            if (isImageEdge) {
              const pulse = document.createElementNS('http://www.w3.org/2000/svg', 'circle');
              pulse.setAttribute('r', '3.5');
              pulse.setAttribute('fill', 'var(--edge-image)');
              pulse.setAttribute('filter', 'url(#blue-glow)');
              const anim = document.createElementNS('http://www.w3.org/2000/svg', 'animateMotion');
              anim.setAttribute('dur', '1.15s');
              anim.setAttribute('repeatCount', 'indefinite');
              anim.setAttribute('path', path);
              pulse.appendChild(anim);
              g.appendChild(pulse);
            }
          }

          // 连线悬浮交互
          g.addEventListener('pointerenter', () => setEdgeHover(conn.id));
          g.addEventListener('pointerleave', () => setEdgeHover(null));
          edgesGroup.appendChild(g);

          // 中点交互徽章
          const badge = document.createElement('div');
          badge.className = `edge-badge ${isImageEdge ? 'badge-image' : ''} ${isHovered ? 'is-hovered' : ''}`;
          badge.style.left = `${midX}px`;
          badge.style.top = `${midY}px`;
          badge.dataset.edgeId = conn.id;
          badge.innerHTML = `
            <span class="badge-dot"></span>
            <span>${esc(roleLabel)}</span>
            <button type="button" class="badge-remove" title="断开此连线">×</button>
          `;
          badge.addEventListener('pointerenter', () => setEdgeHover(conn.id));
          badge.addEventListener('pointerleave', () => setEdgeHover(null));
          badge.addEventListener('pointerdown', e => e.stopPropagation());
          badge.querySelector('.badge-remove').addEventListener('click', (e) => {
            e.stopPropagation();
            disconnectEdge(conn.id);
          });
          edgeBadgesLayer.appendChild(badge);
        });

        drawMinimap();
      }

      function setEdgeHover(edgeId) {
        state.hoveredEdgeId = edgeId;
        edgesGroup.querySelectorAll('.edge-group').forEach(g => {
          g.classList.toggle('is-hovered', g.dataset.edgeId === edgeId);
        });
        edgeBadgesLayer.querySelectorAll('.edge-badge').forEach(b => {
          b.classList.toggle('is-hovered', b.dataset.edgeId === edgeId);
        });
      }

      function connectNodes(fromId, toId) {
        if (!fromId || !toId || fromId === toId) return false;
        const src = state.nodes.find(n => n.id === fromId);
        const dst = state.nodes.find(n => n.id === toId);
        if (!src || !dst) return false;
        if (dst.type === 'text') {
          showToast('文本节点仅作为提示词输出源', 'error');
          return false;
        }
        if (src.type === 'video') {
          showToast('视频节点暂不支持作为上游输入', 'error');
          return false;
        }
        if (state.connections.some(c => c.from === fromId && c.to === toId)) {
          showToast('该连线已存在');
          return false;
        }
        state.connections.push({ id: genId('e'), from: fromId, to: toId });
        pushHistory();
        refreshNodeContent(dst);
        renderEdges();
        updateFloatingUI();
        return true;
      }

      function disconnectEdge(edgeId) {
        const edge = state.connections.find(c => c.id === edgeId);
        if (!edge) return;
        state.connections = state.connections.filter(c => c.id !== edgeId);
        state.hoveredEdgeId = null;
        pushHistory();
        const dst = state.nodes.find(n => n.id === edge.to);
        if (dst) refreshNodeContent(dst);
        renderEdges();
        updateFloatingUI();
        showToast('已断开连线');
      }

      /* ========================================================================
         10. 节点创建、渲染与内部组件
         ======================================================================== */
      function createNode(opts, recordHistory = true) {
        const center = screenToWorld(window.innerWidth / 2, window.innerHeight * 0.42);
        const defAcc = state.accounts[0]?.id || 'acc_pro';
        const defFmt = state.accounts[0]?.responseFormat || 'url';

        const presets = {
          text: {
            title: '文本',
            width: 272,
            content: '',
          },
          image: {
            title: '图片',
            accountId: defAcc,
            aspectRatio: '16:9',
            responseFormat: defFmt,
            prompt: '',
            cameraTags: [],
            localRefs: [],
            outputUrl: '',
            isGenerating: false,
            genStage: '',
            elapsed: 0,
            lastMeta: null,
            error: '',
          },
          video: {
            title: '视频',
            accountId: defAcc,
            aspectRatio: '16:9',
            duration: 6,
            prompt: '',
            cameraTags: [],
            localRefs: [],
            outputUrl: '',
            isMockVideo: false,
            isGenerating: false,
            taskId: '',
            status: '',
            progress: 0,
            elapsed: 0,
            lastMeta: null,
            error: '',
          },
        };

        const base = presets[opts.type] || presets.text;
        const node = {
          ...base,
          ...opts,
          id: opts.id || genId('n'),
          z: ++state.nextZ,
        };
        const dims = getNodeDimensions(node);
        if (opts.x === undefined) node.x = Math.round(center.x - dims.w / 2 + (Math.random() - 0.5) * 30);
        if (opts.y === undefined) node.y = Math.round(center.y - dims.h / 2 + (Math.random() - 0.5) * 30);

        state.nodes.push(node);
        renderNode(node);
        if (recordHistory) pushHistory();
        drawMinimap();
        return node;
      }

      function duplicateNode(node) {
        const clone = {
          ...node,
          id: genId('n'),
          x: node.x + 36,
          y: node.y + 36,
          localRefs: (node.localRefs || []).map(r => ({ ...r, id: genId('r') })),
          cameraTags: [...(node.cameraTags || [])],
          isGenerating: false,
          error: '',
        };
        const created = createNode(clone, true);
        setSelection([created.id]);
        showToast('已复制节点');
      }

      function deleteSelectedNodes() {
        if (state.selectedIds.size === 0) return;
        const toDel = new Set(state.selectedIds);
        toDel.forEach(id => {
          cancelTask(id);

          document.getElementById(id)?.remove();
        });
        state.nodes = state.nodes.filter(n => !toDel.has(n.id));
        state.connections = state.connections.filter(c => !toDel.has(c.from) && !toDel.has(c.to));
        state.selectedIds.clear();
        closePopovers();
        pushHistory();
        state.nodes.forEach(n => { if (n.type !== 'text') refreshNodeContent(n); });
        renderEdges();
        updateFloatingUI();
        showToast(`已删除 ${toDel.size} 个节点`);
      }

      function cancelTask(nodeId) {
        const key = taskKey(library.active, nodeId);
        const t = activeTasks.get(key);
        if (t) {
          post({kind:'cancel', canvasId:library.active, nodeId, requestId:t.requestId});
          t.cancelled = true;
          try { t.abort?.abort(); } catch (_) {}
          if (t.timer) clearInterval(t.timer);
          activeTasks.delete(key);
          const node = state.nodes.find(n => n.id === nodeId);
          if (node) { node.isGenerating = false; node.requestId = ''; node.error = '已停止'; refreshNodeContent(node); updateFloatingUI(); renderEdges(); }
        }
      }

      function setSelection(ids) {
        state.selectedIds = new Set(ids);
        document.querySelectorAll('.canvas-node').forEach(el => {
          el.classList.toggle('is-selected', state.selectedIds.has(el.id));
        });
        if (ids.length === 1) {
          const n = state.nodes.find(x => x.id === ids[0]);
          if (n) {
            n.z = ++state.nextZ;
            const dom = document.getElementById(n.id);
            if (dom) dom.style.zIndex = n.z;
          }
        }
        closePopovers();
        updateFloatingUI();
        drawMinimap();
      }

      function renderNode(node) {
        document.getElementById(node.id)?.remove();

        const dims = getNodeDimensions(node);
        const el = document.createElement('div');
        el.id = node.id;
        el.className = `canvas-node ${state.selectedIds.has(node.id) ? 'is-selected' : ''}`;
        el.style.left = `${node.x}px`;
        el.style.top = `${node.y}px`;
        el.style.width = `${dims.w}px`;
        if (node.type !== 'text') el.style.height = `${dims.h}px`;
        el.style.zIndex = node.z;

        // 外部左上角浮动标签
        const extLabel = document.createElement('div');
        extLabel.className = 'node-external-label';
        const labelInput = document.createElement('input');
        labelInput.type = 'text';
        labelInput.className = 'node-label-input';
        labelInput.value = node.title || (node.type === 'text' ? '文本' : node.type === 'image' ? '图片' : '视频');
        labelInput.addEventListener('pointerdown', e => {
          if (document.activeElement === labelInput) e.stopPropagation();
        });
        labelInput.addEventListener('dblclick', e => {
          e.stopPropagation();
          labelInput.focus();
          labelInput.select();
        });
        labelInput.addEventListener('input', e => {
          node.title = e.target.value;
          updateFloatingUI();
        });
        extLabel.appendChild(labelInput);
        el.appendChild(extLabel);

        // 内容容器
        const contentWrap = document.createElement('div');
        contentWrap.className = 'node-content-wrap';
        contentWrap.style.width = '100%';
        contentWrap.style.height = '100%';
        el.appendChild(contentWrap);

        // 连接端口
        if (node.type !== 'text') {
          el.appendChild(createPortEl(node, 'in'));
        }
        if (node.type !== 'video') {
          el.appendChild(createPortEl(node, 'out'));
        }

        // 节点点击与拖拽
        el.addEventListener('pointerdown', (e) => {
          if (state.isSpaceDown || e.button === 1) return;
          if (e.target.closest('.node-port, .vid-ctrl-btn, .vid-scrubber, .icon-circle-btn, .gen-cancel-btn')) return;
          if (el.classList.contains('is-editing') && e.target.closest('.text-node-editor')) return;

          e.stopPropagation();
          closePopovers();

          if (e.shiftKey) {
            const next = new Set(state.selectedIds);
            if (next.has(node.id)) next.delete(node.id);
            else next.add(node.id);
            setSelection([...next]);
            return;
          }

          if (!state.selectedIds.has(node.id)) {
            setSelection([node.id]);
          }

          interact.mode = 'drag-nodes';
          interact.startX = e.clientX;
          interact.startY = e.clientY;
          interact.movedDuringDrag = false;
          interact.dragOrigins.clear();
          state.selectedIds.forEach(sid => {
            const sn = state.nodes.find(x => x.id === sid);
            if (sn) interact.dragOrigins.set(sid, { x: sn.x, y: sn.y });
          });
        });

        // 节点文件拖放（图片作为参考图，视频替换视频）
        if (node.type === 'image' || node.type === 'video') {
          el.addEventListener('dragover', (e) => {
            if (!e.dataTransfer?.types?.includes('Files')) return;
            e.preventDefault();
            e.stopPropagation();
            el.classList.add('is-drag-over');
          });
          el.addEventListener('dragleave', (e) => {
            if (!el.contains(e.relatedTarget)) el.classList.remove('is-drag-over');
          });
          el.addEventListener('drop', (e) => {
            e.preventDefault();
            e.stopPropagation();
            el.classList.remove('is-drag-over');
            interact.dragDepth = 0;
            dropOverlay.classList.remove('is-open');
            const files = Array.from(e.dataTransfer?.files || []);
            const imgs = files.filter(f => f.type.startsWith('image/'));
            const vids = files.filter(f => f.type.startsWith('video/'));
            if (node.type === 'video' && vids.length) {
              setLocalVideo(node, vids[0]);
            } else if (imgs.length) {
              if (node.type === 'image' && !node.outputUrl && imgs.length === 1) {
                setLocalImage(node, imgs[0]);
              } else {
                addLocalRefs(node, imgs);
              }
            }
          });
        }

        nodesLayer.appendChild(el);
        refreshNodeContent(node);
      }

      function createPortEl(node, side) {
        const port = document.createElement('div');
        port.className = `node-port port-${side}`;
        port.dataset.nodeId = node.id;
        port.dataset.port = side;
        port.title = side === 'out' ? '拖拽输出连线' : '拖拽接入连线';

        port.addEventListener('pointerdown', (e) => {
          if (e.button !== 0) return;
          e.stopPropagation();
          closePopovers();
          interact.mode = 'connect';
          interact.connectFromNodeId = node.id;
          interact.connectFromPort = side;
          interact.snapTarget = null;
          port.classList.add('port-active');
          document.body.classList.add('is-connecting');

          const p = getPortWorldPos(node, side);
          tempEdge.setAttribute('d', computeBezier(p.x, p.y, p.x, p.y).path);
          tempEdge.style.display = 'block';
        });
        return port;
      }

      function refreshNodeContent(node) {
        const el = document.getElementById(node.id);
        if (!el) return;
        const dims = getNodeDimensions(node);
        el.style.width = `${dims.w}px`;
        if (node.type !== 'text') el.style.height = `${dims.h}px`;

        const wrap = el.querySelector('.node-content-wrap');
        if (!wrap) return;

        if (node.type === 'text') {
          renderTextContent(node, el, wrap);
        } else if (node.type === 'image') {
          renderImageContent(node, el, wrap);
        } else if (node.type === 'video') {
          renderVideoContent(node, el, wrap);
        }
      }

      function renderTextContent(node, el, wrap) {
        wrap.innerHTML = `
          <div class="text-node-body">
            <div class="text-node-display ${node.content ? '' : 'is-empty'}">${esc(node.content || '双击编辑文字')}</div>
            <textarea class="text-node-editor thin-scroll" placeholder="双击编辑文字">${esc(node.content || '')}</textarea>
            <div class="text-char-count">${(node.content || '').length} 字</div>
          </div>
        `;
        const display = wrap.querySelector('.text-node-display');
        const editor = wrap.querySelector('.text-node-editor');
        const countEl = wrap.querySelector('.text-char-count');

        const enterEdit = () => {
          el.classList.add('is-editing');
          editor.focus();
          editor.setSelectionRange(editor.value.length, editor.value.length);
        };

        display.addEventListener('dblclick', (e) => {
          e.stopPropagation();
          enterEdit();
        });
        display.addEventListener('click', (e) => {
          if (state.selectedIds.has(node.id) && state.selectedIds.size === 1 && !interact.movedDuringDrag) {
            e.stopPropagation();
            enterEdit();
          }
        });

        editor.addEventListener('input', (e) => {
          node.content = e.target.value;
          scheduleSave();
          display.textContent = node.content || '双击编辑文字';
          display.classList.toggle('is-empty', !node.content);
          countEl.textContent = `${node.content.length} 字`;
          renderEdges();
          updateFloatingUI();
        });
        editor.addEventListener('blur', () => {
          el.classList.remove('is-editing');
          pushHistory();
        });
        editor.addEventListener('wheel', (e) => {
          if (editor.scrollHeight > editor.clientHeight) e.stopPropagation();
        }, { passive: true });
      }

      function renderImageContent(node, el, wrap) {
        const refs = getAllRefs(node);
        const modeText = refs.length > 0 ? '图生图' : '文生图';

        if (node.isGenerating) {
          wrap.innerHTML = `
            <div class="media-stage">
              ${node.outputUrl ? `<img class="media-img" src="${esc(node.outputUrl)}" style="opacity:0.35;filter:blur(4px);" />` : ''}
              <div class="gen-overlay">
                <div class="gen-spinner"></div>
                <div class="gen-stage-text">${esc(node.genStage || '等待 Muse 生成图片…')}</div>
                <div class="gen-meta-row">
                  <span>${node.aspectRatio}</span>
                  <span>·</span>
                  <span class="gen-elapsed">${node.elapsed || 0}s</span>
                </div>
              </div>
            </div>
          `;
          return;
        }

        if (node.outputUrl) {
          wrap.innerHTML = `
            <div class="media-stage">
              <img class="media-img" src="${esc(node.outputUrl)}" alt="${esc(node.title)}" />
              <div class="media-badges">
                <span class="media-badge">${modeText}</span>
                <span class="media-badge mono">${node.aspectRatio}</span>
              </div>
              <div class="media-hover-tools">
                <button class="icon-circle-btn btn-zoom-media" type="button" title="全屏预览">
                  <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="15 3 21 3 21 9"></polyline><polyline points="9 21 3 21 3 15"></polyline><line x1="21" y1="3" x2="14" y2="10"></line><line x1="3" y1="21" x2="10" y2="14"></line></svg>
                </button>
                <a class="icon-circle-btn btn-dl-media" href="${esc(node.outputUrl)}" download="${esc(node.title || 'muse_image')}.png" title="下载图片">
                  <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"></path><polyline points="7 10 12 15 17 10"></polyline><line x1="12" y1="15" x2="12" y2="3"></line></svg>
                </a>
              </div>
            </div>
          `;
          wrap.querySelector('.btn-zoom-media').addEventListener('click', (e) => {
            e.stopPropagation();
            openLightbox('image', node.outputUrl, node.title);
          });
          wrap.querySelector('.btn-dl-media').addEventListener('click', e => e.stopPropagation());
          return;
        }

        wrap.innerHTML = `
          <div class="media-stage">
            <div class="media-empty">
              <div class="media-empty-icon">
                <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6"><rect x="3" y="3" width="18" height="18" rx="3"></rect><circle cx="8.5" cy="8.5" r="1.5"></circle><polyline points="21 15 16 10 5 21"></polyline></svg>
              </div>
              <div class="media-empty-label">空图片节点</div>
            </div>
          </div>
        `;
      }

      function renderVideoContent(node, el, wrap) {
        const refs = getAllRefs(node);
        const modeText = refs.length > 0 ? '图生视频' : '文生视频';

        if (node.isGenerating) {
          const existing = wrap.querySelector('.gen-overlay');
          if (existing) {
            const st = existing.querySelector('.gen-stage-text');
            const bar = existing.querySelector('.gen-progress-bar');
            const elap = existing.querySelector('.gen-elapsed');
            const tid = existing.querySelector('.gen-task-id');
            if (st) st.textContent = node.genStage || '等待 Muse 生成视频…';
            if (bar) bar.style.width = `${clamp(node.progress || 8, 6, 100)}%`;
            if (elap) elap.textContent = `${node.elapsed || 0}s`;
            if (tid && node.taskId) tid.textContent = node.taskId;
            return;
          }

          wrap.innerHTML = `
            <div class="media-stage">
              <div class="gen-overlay">
                <div class="gen-spinner"></div>
                <div class="gen-stage-text">等待 Muse 生成视频…</div>
                <div class="gen-progress-track">
                  <div class="gen-progress-bar" style="width:${clamp(node.progress || 10, 6, 100)}%"></div>
                </div>
                <div class="gen-meta-row">
                  <span class="gen-task-id">${esc(node.taskId || 'vid_pending')}</span>
                  <span>·</span>
                  <span class="gen-elapsed">${node.elapsed || 0}s</span>
                </div>
                <button class="gen-cancel-btn" type="button">取消任务</button>
              </div>
            </div>
          `;
          wrap.querySelector('.gen-cancel-btn').addEventListener('click', (e) => {
            e.stopPropagation();
            cancelTask(node.id);
            node.isGenerating = false;
            node.status = '';
            refreshNodeContent(node);
            renderEdges();
            updateFloatingUI();
            showToast('已取消视频任务');
          });
          return;
        }

        if (node.outputUrl) {

          wrap.innerHTML = `
            <div class="media-stage">
              <video class="video-el" src="${esc(node.outputUrl)}" playsinline loop muted></video>
              <div class="media-badges">
                <span class="media-badge">${modeText}</span>
                <span class="media-badge mono">${node.aspectRatio} · ${node.duration}s</span>
              </div>
              <div class="video-controls">
                <button class="vid-ctrl-btn btn-vid-play" type="button" title="播放/暂停" aria-label="播放">${uiIcon('play')}</button>
                <div class="vid-scrubber" title="拖动进度">
                  <div class="vid-scrubber-track"><div class="vid-scrubber-fill"></div></div>
                </div>
                <span class="vid-time-pill">00:00 / 00:${String(node.duration || 6).padStart(2, '0')}</span>
                <button class="vid-ctrl-btn btn-vid-mute" type="button" title="静音/取消静音" aria-label="取消静音">${uiIcon('muted')}</button>
                <button class="vid-ctrl-btn btn-vid-full" type="button" title="全屏预览" aria-label="全屏预览">${uiIcon('expand')}</button>
              </div>
            </div>
          `;

          const playBtn = wrap.querySelector('.btn-vid-play');
          const muteBtn = wrap.querySelector('.btn-vid-mute');
          const fullBtn = wrap.querySelector('.btn-vid-full');
          const scrubber = wrap.querySelector('.vid-scrubber');

          {
            const vid = wrap.querySelector('.video-el');
            const fill = wrap.querySelector('.vid-scrubber-fill');
            const timeEl = wrap.querySelector('.vid-time-pill');
            const updatePlaybackIcon = () => {
              playBtn.innerHTML = uiIcon(vid.paused ? 'play' : 'pause');
              playBtn.setAttribute('aria-label', vid.paused ? '播放' : '暂停');
            };
            vid.addEventListener('play', updatePlaybackIcon);
            vid.addEventListener('pause', updatePlaybackIcon);
            vid.addEventListener('ended', updatePlaybackIcon);
            playBtn.addEventListener('click', (e) => {
              e.stopPropagation();
              if (vid.paused) vid.play().catch(updatePlaybackIcon);
              else vid.pause();
            });
            muteBtn.addEventListener('click', (e) => {
              e.stopPropagation();
              vid.muted = !vid.muted;
              muteBtn.innerHTML = uiIcon(vid.muted ? 'muted' : 'volume');
              muteBtn.setAttribute('aria-label', vid.muted ? '取消静音' : '静音');
            });
            vid.addEventListener('timeupdate', () => {
              const dur = vid.duration || node.duration || 6;
              fill.style.width = `${clamp((vid.currentTime / dur) * 100, 0, 100)}%`;
              timeEl.textContent = `00:${String(Math.floor(vid.currentTime)).padStart(2, '0')} / 00:${String(Math.floor(dur)).padStart(2, '0')}`;
            });
            scrubber.addEventListener('pointerdown', (e) => {
              e.stopPropagation();
              const rect = scrubber.getBoundingClientRect();
              const ratio = clamp((e.clientX - rect.left) / rect.width, 0, 1);
              if (vid.duration) vid.currentTime = ratio * vid.duration;
            });
            fullBtn.addEventListener('click', (e) => {
              e.stopPropagation();
              openLightbox('video', node.outputUrl, node.title);
            });
          }
          return;
        }

        wrap.innerHTML = `
          <div class="media-stage">
            <div class="media-empty">
              <div class="media-empty-icon">
                <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6"><polygon points="23 7 16 12 23 17 23 7"></polygon><rect x="1" y="5" width="15" height="14" rx="2"></rect></svg>
              </div>
              <div class="media-empty-label">空视频节点</div>
            </div>
          </div>
        `;
      }

      /* ========================================================================
         11. 上下文悬浮 UI（顶部操作胶囊 + 底部悬浮生成面板）
         ======================================================================== */
      function getSingleSelectedNode() {
        if (state.selectedIds.size !== 1) return null;
        const id = [...state.selectedIds][0];
        return state.nodes.find(n => n.id === id) || null;
      }

      function updateFloatingUI() {
        const node = getSingleSelectedNode();
        if (!node || interact.mode === 'drag-nodes' || interact.mode === 'marquee') {
          topActionPill.style.display = 'none';
          genPanel.style.display = 'none';
          nodeInfoPopover.style.display = 'none';
          return;
        }

        const dims = getNodeDimensions(node);
        const topLeft = worldToScreen(node.x, node.y);
        const botRight = worldToScreen(node.x + dims.w, node.y + dims.h);
        const centerX = (topLeft.x + botRight.x) / 2;

        // 1. 顶部操作胶囊（节点上方 12px）
        topActionPill.style.display = 'inline-flex';

        const hasMedia = (node.type === 'image' || node.type === 'video') && Boolean(node.outputUrl);
        const typeBtn = node.type === 'text'
          ? `<button class="pill-action-btn" data-act="copy-text" type="button">复制文本</button>`
          : node.type === 'image'
          ? `<button class="pill-action-btn" data-act="upload-img" type="button">上传图片</button>`
          : `<button class="pill-action-btn" data-act="upload-vid" type="button">上传视频</button>`;

        topActionPill.innerHTML = `
          <button class="pill-action-btn" data-act="info" type="button">信息</button>
          <button class="pill-action-btn" data-act="dup" type="button">复制</button>
          ${typeBtn}
          ${hasMedia ? `<button class="pill-action-btn" data-act="download" type="button">下载</button>` : ''}
          <div class="pill-divider"></div>
          <button class="pill-action-btn danger" data-act="delete" type="button">删除</button>
        `;
        topActionPill.style.left = `${snapPixel(clamp(centerX - topActionPill.offsetWidth / 2, 12, window.innerWidth - topActionPill.offsetWidth - 12))}px`;
        topActionPill.style.top = `${snapPixel(Math.max(topLeft.y - 30 - topActionPill.offsetHeight, 12))}px`;

        topActionPill.querySelectorAll('.pill-action-btn').forEach(btn => {
          btn.addEventListener('click', (e) => {
            e.stopPropagation();
            handleTopAction(node, btn.dataset.act, btn);
          });
        });

        // 2. 底部悬浮生成面板（仅图片/视频节点，锚定于下方 14px）
        if (node.type === 'image' || node.type === 'video') {
          genPanel.style.display = 'block';
          populateGenPanel(node);
          const panelW = Math.min(472, window.innerWidth - 24);
          const safeX = clamp(centerX, panelW / 2 + 12, window.innerWidth - panelW / 2 - 12);
          const panelH = genPanel.offsetHeight || 195;
          const desiredY = botRight.y + 14;
          const safeY = clamp(desiredY, 68, window.innerHeight - panelH - 72);
          genPanel.style.left = `${snapPixel(safeX - panelW / 2)}px`;
          genPanel.style.top = `${snapPixel(safeY)}px`;
        } else {
          genPanel.style.display = 'none';
        }
      }

      function populateGenPanel(node) {
        const refs = getAllRefs(node);
        const { texts } = getUpstream(node.id);
        const acc = getAccount(node.accountId);

        // 接口路径提示
        panelEndpointHint.textContent = '桌面账号 · 本地保存';

        // 参考内容横向条
        panelRefStrip.innerHTML = `
          <button class="ref-add-box" id="btn-panel-add-ref" type="button" title="上传本地参考图">+</button>
        `;
        panelRefStrip.querySelector('#btn-panel-add-ref').addEventListener('click', (e) => {
          e.stopPropagation();
          interact.fileTargetNodeId = node.id;
          interact.fileTargetMode = 'ref';
          fileInputImage.value = '';
          fileInputImage.click();
        });

        refs.forEach((r, idx) => {
          const thumb = document.createElement('div');
          thumb.className = 'ref-thumb';
          const isFirstFrame = node.type === 'video' && idx === 0;
          const tagHtml = isFirstFrame
            ? `<span class="ref-tag first-frame">首帧</span>`
            : r.source === 'upstream'
            ? `<span class="ref-tag">上游</span>`
            : `<span class="ref-tag">#${idx + 1}</span>`;
          const rmHtml = r.source === 'local'
            ? `<button class="ref-rm-btn" type="button" title="移除参考图">×</button>`
            : '';
          thumb.innerHTML = `<img src="${esc(r.url)}" alt="ref" />${tagHtml}${rmHtml}`;
          if (r.source === 'local') {
            thumb.querySelector('.ref-rm-btn').addEventListener('click', (e) => {
              e.stopPropagation();
              node.localRefs = (node.localRefs || []).filter(x => x.id !== r.id);
              pushHistory();
              refreshNodeContent(node);
              updateFloatingUI();
            });
          }
          panelRefStrip.appendChild(thumb);
        });

        // 上游文本连入胶囊
        panelUpstreamPills.innerHTML = '';
        panelUpstreamPills.style.display = texts.length ? 'flex' : 'none';
        texts.forEach(t => {
          const pill = document.createElement('button');
          pill.type = 'button';
          pill.className = 'upstream-pill';
          pill.title = t.text || '（空文本）';
          pill.innerHTML = `${uiIcon('prompt')}<span>${esc(t.title)}</span>`;
          pill.addEventListener('click', (e) => {
            e.stopPropagation();
            setSelection([t.nodeId]);
          });
          panelUpstreamPills.appendChild(pill);
        });

        // 提示词输入框（仅在非聚焦时同步值，避免光标跳动）
        panelPromptInput.placeholder = node.type === 'image'
          ? '描述要生成的图片内容（已连入的文本节点会自动合并）'
          : '描述要生成的视频内容与运镜动作';
        if (document.activeElement !== panelPromptInput) {
          panelPromptInput.value = node.prompt || '';
        }

        // 错误提示
        if (node.error) {
          panelErrorBanner.textContent = node.error;
          panelErrorBanner.style.display = 'block';
        } else {
          panelErrorBanner.style.display = 'none';
        }

        // 账号、参数、摄像机按钮文本
        panelAccountLabel.textContent = acc.name;
        if (node.type === 'image') {
          const fmt = node.responseFormat || acc.responseFormat || 'url';
          const modeLabel = refs.length > 0 ? '图生图' : '文生图';
          panelParamsLabel.textContent = `${node.aspectRatio} · ${modeLabel}`;
        } else {
          const modeLabel = refs.length > 0 ? '图生视频' : '文生视频';
          panelParamsLabel.textContent = `${node.aspectRatio} · ${node.duration}s · ${modeLabel}`;
        }

        const camCount = (node.cameraTags || []).length;
        panelCameraLabel.textContent = camCount > 0 ? `摄像机 (${camCount})` : '摄像机';
        btnPanelCamera.classList.toggle('is-active', camCount > 0);

        btnPanelGenerate.disabled = false;
        btnPanelGenerate.innerHTML = node.isGenerating
          ? `<span>停止生成</span>`
          : `<span>生成</span><svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 19V5M5 12l7 -7l7 7"/></svg>`;
      }

      function handleTopAction(node, action, anchorBtn) {
        if (action === 'delete') {
          deleteSelectedNodes();
        } else if (action === 'dup') {
          duplicateNode(node);
        } else if (action === 'copy-text') {
          copyTextReliable(node.content || '');
        } else if (action === 'upload-img') {
          interact.fileTargetNodeId = node.id;
          interact.fileTargetMode = 'replace-image';
          fileInputImage.value = '';
          fileInputImage.click();
        } else if (action === 'upload-vid') {
          interact.fileTargetNodeId = node.id;
          interact.fileTargetMode = 'replace-video';
          fileInputVideo.value = '';
          fileInputVideo.click();
        } else if (action === 'download') {
          if (!node.outputUrl) return;
          const a = document.createElement('a');
          a.href = node.outputUrl;
          a.download = `${(node.title || 'muse').replace(/\s+/g, '_')}.${node.type === 'video' ? 'mp4' : 'png'}`;
          a.click();
        } else if (action === 'info') {
          toggleInfoPopover(node, anchorBtn);
        }
      }

      function toggleInfoPopover(node, anchorBtn) {
        if (nodeInfoPopover.style.display === 'block') {
          nodeInfoPopover.style.display = 'none';
          return;
        }
        const dims = getNodeDimensions(node);
        const acc = getAccount(node.accountId);
        const refs = node.type === 'text' ? [] : getAllRefs(node);
        const endpoint = node.type === 'text' ? '上游提示词输出' : 'Muse 桌面账号';

        nodeInfoPopover.innerHTML = `
          <div class="info-title">
            <span>节点元数据</span>
            <span style="font-size:10px;font-family:var(--font-mono);color:var(--text-muted);">${esc(node.id)}</span>
          </div>
          <div class="info-grid">
            <div class="info-row"><span class="info-k">类型 / 尺寸</span><span class="info-v">${node.type} · ${dims.w}×${dims.h}</span></div>
            ${node.type !== 'text' ? `<div class="info-row"><span class="info-k">绑定账号</span><span class="info-v">${esc(acc.name)}</span></div>` : ''}
            <div class="info-row"><span class="info-k">执行方式</span><span class="info-v">${esc(endpoint)}</span></div>
            ${node.type === 'image' ? `<div class="info-row"><span class="info-k">画幅 / 格式</span><span class="info-v">${node.aspectRatio} · ${node.responseFormat || acc.responseFormat || 'url'}</span></div>` : ''}
            ${node.type === 'video' ? `<div class="info-row"><span class="info-k">画幅 / 时长</span><span class="info-v">${node.aspectRatio} · ${node.duration}s</span></div>` : ''}
            ${node.taskId ? `<div class="info-row"><span class="info-k">最近 Task ID</span><span class="info-v">${esc(node.taskId)}</span></div>` : ''}
            ${node.lastMeta ? `<div class="info-row"><span class="info-k">最近生成</span><span class="info-v">${esc(node.lastMeta)}</span></div>` : ''}
          </div>
        `;

        const rect = anchorBtn.getBoundingClientRect();
        nodeInfoPopover.style.display = 'block';
        nodeInfoPopover.style.left = `${clamp(rect.left, 12, window.innerWidth - 290)}px`;
        nodeInfoPopover.style.top = `${Math.min(rect.bottom + 8, window.innerHeight - 210)}px`;
      }

      /* ========================================================================
         12. 面板内联弹出菜单（预设 / 账号 / 参数 / 摄像机 / JSON 导入导出）
         ======================================================================== */
      let activePopoverKey = null;

      function closePopovers() {
        inlinePopover.style.display = 'none';
        quickCreateMenu.style.display = 'none';
        nodeInfoPopover.style.display = 'none';
        activePopoverKey = null;
      }

      function openInlinePopover(key, anchorEl, renderFn) {
        if (activePopoverKey === key && inlinePopover.style.display === 'block') {
          closePopovers();
          return;
        }
        closePopovers();
        activePopoverKey = key;
        inlinePopover.innerHTML = '';
        renderFn(inlinePopover);
        inlinePopover.style.display = 'block';

        const r = anchorEl.getBoundingClientRect();
        const pw = inlinePopover.offsetWidth || 220;
        const ph = inlinePopover.offsetHeight || 160;
        const left = clamp(r.left, 12, window.innerWidth - pw - 12);
        const top = (r.top - ph - 8 > 12) ? (r.top - ph - 8) : Math.min(r.bottom + 8, window.innerHeight - ph - 12);
        inlinePopover.style.left = `${left}px`;
        inlinePopover.style.top = `${top}px`;
      }

      // 提示词输入同步
      panelPromptInput.addEventListener('input', (e) => {
        const node = getSingleSelectedNode();
        if (!node || node.type === 'text') return;
        node.prompt = e.target.value;
        scheduleSave();
      });
      panelPromptInput.addEventListener('blur', () => pushHistory());

      // 灵感扩写
      document.getElementById('btn-panel-expand').addEventListener('click', (e) => {
        e.stopPropagation();
        const node = getSingleSelectedNode();
        if (!node || node.type === 'text') return;
        const base = (node.prompt || '').trim();
        const suffix = node.type === 'image'
          ? '超精细写实材质，电影级冷暖光影层次，柔和体积光与景深虚化，无水印纯净画面'
          : '平滑电影级运镜，24fps 自然动态模糊，细腻空气尘埃粒子与光影渐变';
        node.prompt = base ? `${base}，${suffix}` : suffix;
        panelPromptInput.value = node.prompt;
        pushHistory();
        showToast('已完成灵感扩写');
      });

      // 提示词预设 Popover
      document.getElementById('btn-panel-presets').addEventListener('click', (e) => {
        e.stopPropagation();
        const node = getSingleSelectedNode();
        if (!node) return;
        openInlinePopover('presets', e.currentTarget, (box) => {
          box.innerHTML = `<div class="popover-header">提示词预设</div>`;
          PROMPT_PRESETS.forEach(p => {
            const btn = document.createElement('button');
            btn.type = 'button';
            btn.className = 'popover-item';
            btn.innerHTML = `<span>${esc(p.name)}</span><span style="font-size:10px;color:var(--text-muted);">填入</span>`;
            btn.addEventListener('click', () => {
              node.prompt = p.text;
              panelPromptInput.value = p.text;
              pushHistory();
              closePopovers();
              showToast(`已应用预设：${p.name}`);
            });
            box.appendChild(btn);
          });
        });
      });

      // 账号切换 Popover
      document.getElementById('btn-panel-account').addEventListener('click', (e) => {
        e.stopPropagation();
        const node = getSingleSelectedNode();
        if (!node) return;
        openInlinePopover('account', e.currentTarget, (box) => {
          box.innerHTML = `<div class="popover-header">选择执行账号</div>`;
          state.accounts.forEach(acc => {
            const btn = document.createElement('button');
            btn.type = 'button';
            btn.className = `popover-item ${node.accountId === acc.id ? 'is-selected' : ''}`;
            btn.innerHTML = `
              <span style="display:flex;align-items:center;gap:6px;"><span class="status-dot ${acc.running ? '' : 'is-offline'}"></span>${esc(acc.name)}</span>
              <span style="font-size:10px;font-family:var(--font-mono);color:var(--text-muted);">${acc.busy ? '忙碌' : acc.running ? '已启动' : '未启动'}</span>
            `;
            btn.addEventListener('click', () => {
              node.accountId = acc.id;
              if (node.type === 'image') node.responseFormat = acc.responseFormat || 'url';
              pushHistory();
              closePopovers();
              updateFloatingUI();
            });
            box.appendChild(btn);
          });
          const mgr = document.createElement('button');
          mgr.type = 'button';
          mgr.className = 'popover-item';
          mgr.style.borderTop = '1px solid var(--divider)';
          mgr.style.marginTop = '4px';
          mgr.innerHTML = `<span class="account-manager-label" style="color:var(--accent-ink);">${uiIcon('settings')}<span>管理账号...</span></span>`;
          mgr.addEventListener('click', () => {
            closePopovers();
            openAccountsModal();
          });
          box.appendChild(mgr);
        });
      });

      // 参数配置 Popover（画幅比例 + 格式/时长）
      document.getElementById('btn-panel-params').addEventListener('click', (e) => {
        e.stopPropagation();
        const node = getSingleSelectedNode();
        if (!node) return;
        openInlinePopover('params', e.currentTarget, (box) => {
          if (node.type === 'image') {
            box.innerHTML = `<div class="popover-header">画幅比例</div><div class="popover-chips" id="pop-ratios"></div>`;
            const rWrap = box.querySelector('#pop-ratios');
            IMAGE_RATIOS.forEach(r => {
              const chip = document.createElement('button');
              chip.type = 'button';
              chip.className = `popover-chip ${node.aspectRatio === r.value ? 'is-selected' : ''}`;
              chip.textContent = r.value;
              chip.addEventListener('click', () => {
                node.aspectRatio = r.value;
                pushHistory();
                refreshNodeContent(node);
                renderEdges();
                updateFloatingUI();
                closePopovers();
              });
              rWrap.appendChild(chip);
            });
          } else {
            box.innerHTML = `<div class="popover-header">视频画幅</div><div class="popover-chips" id="pop-vratios"></div><div class="popover-header">生成时长（Muse 上游）</div><div class="popover-chips" id="pop-vdurs"></div>`;
            const vr = box.querySelector('#pop-vratios');
            VIDEO_RATIOS.forEach(r => {
              const chip = document.createElement('button');
              chip.type = 'button';
              chip.className = `popover-chip ${node.aspectRatio === r.value ? 'is-selected' : ''}`;
              chip.textContent = r.label;
              chip.addEventListener('click', () => {
                node.aspectRatio = r.value;
                pushHistory();
                refreshNodeContent(node);
                renderEdges();
                updateFloatingUI();
                closePopovers();
              });
              vr.appendChild(chip);
            });
            const vd = box.querySelector('#pop-vdurs');
            VIDEO_DURATIONS.forEach(d => {
              const chip = document.createElement('button');
              chip.type = 'button';
              chip.className = `popover-chip ${node.duration === d ? 'is-selected' : ''}`;
              chip.style.fontFamily = 'var(--font-mono)';
              chip.textContent = `${d}s`;
              chip.addEventListener('click', () => {
                node.duration = d;
                pushHistory();
                refreshNodeContent(node);
                updateFloatingUI();
                closePopovers();
              });
              vd.appendChild(chip);
            });
          }
        });
      });

      // 摄像机 / 镜头标签 Popover
      btnPanelCamera.addEventListener('click', (e) => {
        e.stopPropagation();
        const node = getSingleSelectedNode();
        if (!node) return;
        openInlinePopover('camera', e.currentTarget, (box) => {
          box.style.width = '252px';
          box.innerHTML = `<div class="popover-header">摄像机 / 镜头与运镜（多选）</div><div class="popover-chips" id="pop-cams"></div>`;
          const wrap = box.querySelector('#pop-cams');
          const renderChips = () => {
            wrap.innerHTML = '';
            CAMERA_TAGS.forEach(tag => {
              const active = (node.cameraTags || []).includes(tag);
              const chip = document.createElement('button');
              chip.type = 'button';
              chip.className = `popover-chip ${active ? 'is-selected' : ''}`;
              chip.textContent = tag;
              chip.addEventListener('click', () => {
                if (!Array.isArray(node.cameraTags)) node.cameraTags = [];
                if (active) node.cameraTags = node.cameraTags.filter(t => t !== tag);
                else node.cameraTags.push(tag);
                pushHistory();
                renderChips();
                updateFloatingUI();
              });
              wrap.appendChild(chip);
            });
          };
          renderChips();
        });
      });

      // 生成按钮触发
      btnPanelGenerate.addEventListener('click', (e) => {
        e.stopPropagation();
        const node = getSingleSelectedNode();
        if (!node) return;
        if (node.isGenerating) { cancelTask(node.id); pushHistory(); return; }
        if (node.type === 'image') runDesktopGeneration(node);
        else if (node.type === 'video') runDesktopGeneration(node);
      });

      /* ========================================================================
         13. 生成请求由桌面任务队列执行
         ======================================================================== */
      /* ========================================================================
         14. 本地媒体上传与拖放
         ======================================================================== */
      async function addLocalRefs(node, files) {
        try {
        if (!Array.isArray(node.localRefs)) node.localRefs = [];
        for (const f of files) {
          const url = await persistFile(f);
          node.localRefs.push({ id: genId('r'), name: f.name, url });
        }
        pushHistory();
        refreshNodeContent(node);
        updateFloatingUI();
        showToast(`已添加 ${files.length} 张参考图`);
        } catch(error) { pushHistory(); refreshNodeContent(node); updateFloatingUI(); showToast(error.message, 'error'); }
      }

      async function setLocalImage(node, file) {
        try {
        node.outputUrl = await persistFile(file);
        node.lastMeta = `本地上传 · ${file.name}`;
        pushHistory();
        refreshNodeContent(node);
        state.connections.filter(c => c.from === node.id).forEach(c => {
          const dst = state.nodes.find(n => n.id === c.to);
          if (dst) refreshNodeContent(dst);
        });
        updateFloatingUI();
        showToast('已载入本地图片');
        } catch(error) { showToast(error.message, 'error'); }
      }

      async function setLocalVideo(node, file) {
        try {

        node.isMockVideo = false;
        node.outputUrl = await persistFile(file);
        node.lastMeta = `本地视频 · ${file.name}`;
        pushHistory();
        refreshNodeContent(node);
        updateFloatingUI();
        showToast('已载入本地视频');
        } catch(error) { showToast(error.message, 'error'); }
      }

      fileInputImage.addEventListener('change', (e) => {
        const files = Array.from(e.target.files || []);
        const node = state.nodes.find(n => n.id === interact.fileTargetNodeId);
        if (!node || !files.length) return;
        if (interact.fileTargetMode === 'replace-image') setLocalImage(node, files[0]);
        else addLocalRefs(node, files);
      });

      fileInputVideo.addEventListener('change', (e) => {
        const file = e.target.files?.[0];
        const node = state.nodes.find(n => n.id === interact.fileTargetNodeId);
        if (node && file) setLocalVideo(node, file);
      });

      /* ========================================================================
         15. 交互式小地图（右下角）
         ======================================================================== */
      let minimapBounds = { minX: 0, minY: 0, scale: 1, ox: 0, oy: 0 };

      function drawMinimap() {
        const w = minimapWrap.clientWidth;
        const h = minimapWrap.clientHeight;
        if (!w || !h) return;
        const dpr = Math.min(window.devicePixelRatio || 1, 2);
        if (minimapCanvas.width !== w * dpr || minimapCanvas.height !== h * dpr) {
          minimapCanvas.width = w * dpr;
          minimapCanvas.height = h * dpr;
        }
        const ctx = minimapCtx;
        ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
        ctx.clearRect(0, 0, w, h);
        const palette = getComputedStyle(document.documentElement);
        const color = token => palette.getPropertyValue(token).trim();

        // 计算包含所有节点与当前视口的世界边界
        const vTopLeft = screenToWorld(0, 0);
        const vBotRight = screenToWorld(window.innerWidth, window.innerHeight);
        let minX = vTopLeft.x, minY = vTopLeft.y, maxX = vBotRight.x, maxY = vBotRight.y;

        state.nodes.forEach(n => {
          const d = getNodeDimensions(n);
          minX = Math.min(minX, n.x - 100);
          minY = Math.min(minY, n.y - 100);
          maxX = Math.max(maxX, n.x + d.w + 100);
          maxY = Math.max(maxY, n.y + d.h + 100);
        });

        const worldW = Math.max(maxX - minX, 400);
        const worldH = Math.max(maxY - minY, 300);
        const pad = 10;
        const s = Math.min((w - pad * 2) / worldW, (h - pad * 2) / worldH);
        const ox = (w - worldW * s) / 2;
        const oy = (h - worldH * s) / 2;
        minimapBounds = { minX, minY, scale: s, ox, oy };

        const toMini = (wx, wy) => ({ x: ox + (wx - minX) * s, y: oy + (wy - minY) * s });

        // 绘制连线
        state.connections.forEach(c => {
          const src = state.nodes.find(n => n.id === c.from);
          const dst = state.nodes.find(n => n.id === c.to);
          if (!src || !dst) return;
          const p1 = toMini(getPortWorldPos(src, 'out').x, getPortWorldPos(src, 'out').y);
          const p2 = toMini(getPortWorldPos(dst, 'in').x, getPortWorldPos(dst, 'in').y);
          ctx.strokeStyle = color(src.type === 'image' ? '--edge-image' : '--edge-text');
          ctx.lineWidth = 1;
          ctx.beginPath();
          ctx.moveTo(p1.x, p1.y);
          ctx.lineTo(p2.x, p2.y);
          ctx.stroke();
        });

        // 绘制节点
        state.nodes.forEach(n => {
          const d = getNodeDimensions(n);
          const pos = toMini(n.x, n.y);
          const nw = Math.max(d.w * s, 5);
          const nh = Math.max(d.h * s, 4);
          ctx.fillStyle = color(state.selectedIds.has(n.id) ? '--accent-blue' : '--minimap-node');
          ctx.beginPath();
          ctx.roundRect ? ctx.roundRect(pos.x, pos.y, nw, nh, 2) : ctx.rect(pos.x, pos.y, nw, nh);
          ctx.fill();
        });

        // 绘制当前视口框
        const vp1 = toMini(vTopLeft.x, vTopLeft.y);
        const vp2 = toMini(vBotRight.x, vBotRight.y);
        ctx.strokeStyle = color('--minimap-frame');
        ctx.fillStyle = color('--minimap-fill');
        ctx.lineWidth = 1;
        ctx.fillRect(vp1.x, vp1.y, vp2.x - vp1.x, vp2.y - vp1.y);
        ctx.strokeRect(vp1.x, vp1.y, vp2.x - vp1.x, vp2.y - vp1.y);
      }

      function panFromMinimap(clientX, clientY) {
        const rect = minimapWrap.getBoundingClientRect();
        const mx = clientX - rect.left;
        const my = clientY - rect.top;
        const { minX, minY, scale, ox, oy } = minimapBounds;
        const wx = minX + (mx - ox) / scale;
        const wy = minY + (my - oy) / scale;
        state.viewport.x = window.innerWidth / 2 - wx * state.viewport.scale;
        state.viewport.y = window.innerHeight / 2 - wy * state.viewport.scale;
        applyViewport();
      }

      minimapWrap.addEventListener('pointerdown', (e) => {
        e.stopPropagation();
        interact.mode = 'minimap';
        panFromMinimap(e.clientX, e.clientY);
      });

      /* ========================================================================
         16. 画布指针、滚轮、框选、连线磁吸与快捷新建交互
         ======================================================================== */
      canvasEl.addEventListener('pointerdown', (e) => {
        closePopovers();
        if (e.button === 1 || (e.button === 0 && state.isSpaceDown)) {
          e.preventDefault();
          interact.mode = 'pan';
          interact.startX = e.clientX;
          interact.startY = e.clientY;
          interact.viewX = state.viewport.x;
          interact.viewY = state.viewport.y;
          canvasEl.classList.add('is-panning');
          return;
        }
        if (e.button === 0 && e.target === canvasEl) {
          if (e.shiftKey) {
            interact.mode = 'marquee';
            interact.startX = e.clientX;
            interact.startY = e.clientY;
            canvasEl.classList.add('is-marquee');
            marqueeBox.style.left = `${e.clientX}px`;
            marqueeBox.style.top = `${e.clientY}px`;
            marqueeBox.style.width = '0px';
            marqueeBox.style.height = '0px';
            marqueeBox.style.display = 'block';
          } else {
            setSelection([]);
            interact.mode = 'pan';
            interact.startX = e.clientX;
            interact.startY = e.clientY;
            interact.viewX = state.viewport.x;
            interact.viewY = state.viewport.y;
            canvasEl.classList.add('is-panning');
          }
        }
      });

      window.addEventListener('pointermove', (e) => {
        if (!interact.mode) return;

        if (interact.mode === 'pan') {
          state.viewport.x = interact.viewX + (e.clientX - interact.startX);
          state.viewport.y = interact.viewY + (e.clientY - interact.startY);
          applyViewport();
        } else if (interact.mode === 'minimap') {
          panFromMinimap(e.clientX, e.clientY);
        } else if (interact.mode === 'drag-nodes') {
          const dx = (e.clientX - interact.startX) / state.viewport.scale;
          const dy = (e.clientY - interact.startY) / state.viewport.scale;
          if (Math.abs(dx) > 2 || Math.abs(dy) > 2) {
            interact.movedDuringDrag = true;
            topActionPill.style.display = 'none';
            genPanel.style.display = 'none';
          }
          interact.dragOrigins.forEach((orig, sid) => {
            const n = state.nodes.find(x => x.id === sid);
            if (!n) return;
            n.x = orig.x + dx;
            n.y = orig.y + dy;
            const dom = document.getElementById(sid);
            if (dom) { dom.style.left = `${n.x}px`; dom.style.top = `${n.y}px`; }
          });
          renderEdges();
        } else if (interact.mode === 'marquee') {
          const x = Math.min(interact.startX, e.clientX);
          const y = Math.min(interact.startY, e.clientY);
          const w = Math.abs(e.clientX - interact.startX);
          const h = Math.abs(e.clientY - interact.startY);
          marqueeBox.style.left = `${x}px`;
          marqueeBox.style.top = `${y}px`;
          marqueeBox.style.width = `${w}px`;
          marqueeBox.style.height = `${h}px`;

          const p1 = screenToWorld(x, y);
          const p2 = screenToWorld(x + w, y + h);
          const hitIds = [];
          state.nodes.forEach(n => {
            const d = getNodeDimensions(n);
            if (n.x + d.w >= p1.x && n.x <= p2.x && n.y + d.h >= p1.y && n.y <= p2.y) {
              hitIds.push(n.id);
            }
          });
          state.selectedIds = new Set(hitIds);
          document.querySelectorAll('.canvas-node').forEach(el => {
            el.classList.toggle('is-selected', state.selectedIds.has(el.id));
          });
        } else if (interact.mode === 'connect') {
          const src = state.nodes.find(x => x.id === interact.connectFromNodeId);
          if (!src) return;
          const pStart = getPortWorldPos(src, interact.connectFromPort);
          let cur = screenToWorld(e.clientX, e.clientY);

          // 磁吸检测兼容端口（半径 36px 世界坐标）
          document.querySelectorAll('.node-port.port-snap').forEach(el => el.classList.remove('port-snap'));
          interact.snapTarget = null;
          const wantSide = interact.connectFromPort === 'out' ? 'in' : 'out';
          let bestDist = 38 / state.viewport.scale;

          state.nodes.forEach(cand => {
            if (cand.id === src.id) return;
            if (wantSide === 'in' && cand.type === 'text') return;
            if (wantSide === 'out' && cand.type === 'video') return;
            const pos = getPortWorldPos(cand, wantSide);
            const dist = Math.hypot(pos.x - cur.x, pos.y - cur.y);
            if (dist < bestDist) {
              bestDist = dist;
              interact.snapTarget = { nodeId: cand.id, port: wantSide, pos };
            }
          });

          if (interact.snapTarget) {
            cur = interact.snapTarget.pos;
            document.getElementById(interact.snapTarget.nodeId)?.querySelector(`.port-${wantSide}`)?.classList.add('port-snap');
          }

          const bez = interact.connectFromPort === 'out'
            ? computeBezier(pStart.x, pStart.y, cur.x, cur.y)
            : computeBezier(cur.x, cur.y, pStart.x, pStart.y);
          tempEdge.setAttribute('d', bez.path);
        }
      });

      window.addEventListener('pointerup', (e) => {
        if (!interact.mode) return;
        const prevMode = interact.mode;
        interact.mode = null;

        if (prevMode === 'pan') {
          canvasEl.classList.remove('is-panning');
        } else if (prevMode === 'marquee') {
          canvasEl.classList.remove('is-marquee');
          marqueeBox.style.display = 'none';
          updateFloatingUI();
          drawMinimap();
        } else if (prevMode === 'drag-nodes') {
          if (interact.movedDuringDrag) pushHistory();
          updateFloatingUI();
          drawMinimap();
        } else if (prevMode === 'connect') {
          tempEdge.style.display = 'none';
          document.body.classList.remove('is-connecting');
          document.querySelectorAll('.node-port.port-active, .node-port.port-snap').forEach(p => {
            p.classList.remove('port-active', 'port-snap');
          });

          if (interact.snapTarget) {
            if (interact.connectFromPort === 'out') {
              connectNodes(interact.connectFromNodeId, interact.snapTarget.nodeId);
            } else {
              connectNodes(interact.snapTarget.nodeId, interact.connectFromNodeId);
            }
          } else {
            // 落在空白画布上：弹出快捷新建菜单并自动连线
            const hitNode = document.elementFromPoint(e.clientX, e.clientY)?.closest?.('.canvas-node');
            if (!hitNode) {
              const wPos = screenToWorld(e.clientX, e.clientY);
              interact.pendingQuickWire = {
                fromNodeId: interact.connectFromNodeId,
                fromPort: interact.connectFromPort,
                worldX: wPos.x,
                worldY: wPos.y,
              };
              quickCreateMenu.style.display = 'block';
              quickCreateMenu.style.left = `${clamp(e.clientX, 12, window.innerWidth - 180)}px`;
              quickCreateMenu.style.top = `${clamp(e.clientY, 12, window.innerHeight - 150)}px`;
            }
          }
          interact.connectFromNodeId = null;
          interact.snapTarget = null;
        }
      });

      // 快捷新建菜单点击
      quickCreateMenu.querySelectorAll('[data-create-type]').forEach(btn => {
        btn.addEventListener('click', (e) => {
          e.stopPropagation();
          const info = interact.pendingQuickWire;
          if (!info) return;
          const type = btn.dataset.createType;
          const newNode = createNode({
            type,
            x: Math.round(info.fromPort === 'out' ? info.worldX : info.worldX - 300),
            y: Math.round(info.worldY - 100),
          }, false);
          if (info.fromPort === 'out') connectNodes(info.fromNodeId, newNode.id);
          else connectNodes(newNode.id, info.fromNodeId);
          setSelection([newNode.id]);
          quickCreateMenu.style.display = 'none';
          interact.pendingQuickWire = null;
        });
      });

      // 鼠标滚轮缩放
      canvasEl.addEventListener('wheel', (e) => {
        e.preventDefault();
        const factor = e.deltaY < 0 ? 1.09 : 0.91;
        zoomAt(state.viewport.scale * factor, e.clientX, e.clientY);
      }, { passive: false });

      // 键盘快捷键
      window.addEventListener('keydown', (e) => {
        if (library.busy) { e.preventDefault(); return; }
        if (isInputFocused()) {
          if (e.key === 'Escape') document.activeElement.blur();
          return;
        }
        if (e.code === 'Space' && !state.isSpaceDown) {
          e.preventDefault();
          state.isSpaceDown = true;
          canvasEl.classList.add('is-panning');
        } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'z') {
          e.preventDefault();
          if (e.shiftKey) redo(); else undo();
        } else if (e.key === 'Delete' || e.key === 'Backspace') {
          if (state.selectedIds.size > 0) {
            e.preventDefault();
            deleteSelectedNodes();
          }
        } else if (e.key === 'Escape') {
          closePopovers();
          accountsModal.classList.remove('is-open');
          lightboxModal.classList.remove('is-open');
          setSelection([]);
        }
      });

      window.addEventListener('keyup', (e) => {
        if (e.code === 'Space') {
          state.isSpaceDown = false;
          if (interact.mode !== 'pan') canvasEl.classList.remove('is-panning');
        }
      });

      // 全局点击关闭弹出层
      window.addEventListener('pointerdown', (e) => {
        if (!e.target.closest('#inline-popover, #quick-create-menu, #node-info-popover, #gen-floating-panel, #top-action-pill, #bottom-dock')) {
          closePopovers();
        }
      });

      // 全局文件拖放至画布
      window.addEventListener('dragenter', (e) => {
        if (!e.dataTransfer?.types?.includes('Files')) return;
        e.preventDefault();
        interact.dragDepth++;
        dropOverlay.classList.add('is-open');
      });
      window.addEventListener('dragover', (e) => {
        if (e.dataTransfer?.types?.includes('Files')) e.preventDefault();
      });
      window.addEventListener('dragleave', (e) => {
        if (!e.dataTransfer?.types?.includes('Files')) return;
        interact.dragDepth = Math.max(0, interact.dragDepth - 1);
        if (interact.dragDepth === 0) dropOverlay.classList.remove('is-open');
      });
      window.addEventListener('drop', (e) => {
        e.preventDefault();
        interact.dragDepth = 0;
        dropOverlay.classList.remove('is-open');
        const files = Array.from(e.dataTransfer?.files || []);
        if (!files.length) return;
        const pos = screenToWorld(e.clientX, e.clientY);
        files.forEach((f, idx) => {
          if (f.type.startsWith('image/')) {
            const n = createNode({
              type: 'image',
              x: pos.x - 160 + idx * 32,
              y: pos.y - 100 + idx * 32,
              title: f.name.replace(/\.[^.]+$/, ''),
              outputUrl: URL.createObjectURL(f),
            });
            setSelection([n.id]);
          } else if (f.type.startsWith('video/')) {
            const n = createNode({
              type: 'video',
              x: pos.x - 160 + idx * 32,
              y: pos.y - 100 + idx * 32,
              title: f.name.replace(/\.[^.]+$/, ''),
              outputUrl: URL.createObjectURL(f),
              isMockVideo: false,
            });
            setSelection([n.id]);
          }
        });
      });

      /* ========================================================================
         17. 账号配置弹窗、Lightbox 与 JSON 导入/导出
         ======================================================================== */
      function openAccountsModal() {
        accountsModal.classList.add('is-open');
        renderAccountsList();
      }
      function renderAccountsList() {
        accountsListEl.replaceChildren();
        if (!state.accounts.length) { accountsListEl.textContent = '暂无桌面账号，请从客户端左侧新增账号。'; return; }
        for (const acc of state.accounts) {
          const row = document.createElement('div');
          row.className = 'popover-item';
          const name = document.createElement('span'); name.textContent = acc.name;
          const status = document.createElement('span'); status.style.color = 'var(--text-secondary)';
          status.textContent = acc.busy ? '任务进行中' : acc.running ? '已启动' : '未启动';
          row.append(name, status); accountsListEl.appendChild(row);
        }
      }
      document.getElementById('btn-open-accounts').addEventListener('click', openAccountsModal);
      document.getElementById('btn-close-accounts').addEventListener('click', () => accountsModal.classList.remove('is-open'));
      accountsModal.addEventListener('click', e => { if(e.target === accountsModal) accountsModal.classList.remove('is-open'); });

      function openLightbox(type, url, title) {
        lightboxContent.innerHTML = type === 'video'
          ? `<video src="${url}" controls autoplay loop style="max-width:86vw;max-height:74vh;display:block;"></video>`
          : `<img src="${url}" alt="${esc(title)}" style="max-width:86vw;max-height:74vh;display:block;object-fit:contain;" />`;
        lightboxDownload.href = url;
        lightboxDownload.download = `${(title || 'muse').replace(/\s+/g, '_')}.${type === 'video' ? 'mp4' : 'png'}`;
        lightboxModal.classList.add('is-open');
      }
      document.getElementById('btn-close-lightbox').addEventListener('click', () => lightboxModal.classList.remove('is-open'));
      lightboxModal.addEventListener('click', (e) => { if (e.target === lightboxModal) lightboxModal.classList.remove('is-open'); });

      // JSON 导入 / 导出
      document.getElementById('btn-json-io').addEventListener('click', (e) => {
        e.stopPropagation();
        openInlinePopover('json-io', e.currentTarget, (box) => {
          box.innerHTML = `<div class="popover-header">工作流 JSON</div>`;
          const expBtn = document.createElement('button');
          expBtn.type = 'button';
          expBtn.className = 'popover-item';
          expBtn.textContent = '导出工作流 JSON';
          expBtn.addEventListener('click', () => {
            const blob = new Blob([serializeGraph()], { type: 'application/json' });
            const url = URL.createObjectURL(blob);
            const a = document.createElement('a');
            a.href = url;
            a.download = `muse_workflow_${Date.now()}.json`;
            a.click();
            URL.revokeObjectURL(url);
            closePopovers();
            showToast('已导出 JSON 文件');
          });
          const impBtn = document.createElement('button');
          impBtn.type = 'button';
          impBtn.className = 'popover-item';
          impBtn.textContent = '导入工作流 JSON';
          impBtn.addEventListener('click', () => {
            closePopovers();
            fileInputJson.value = '';
            fileInputJson.click();
          });
          box.appendChild(expBtn);
          box.appendChild(impBtn);
        });
      });

      fileInputJson.addEventListener('change', (e) => {
        const destinationCanvas = library.active;
        const f = e.target.files?.[0];
        if (!f) return;
        if (f.size > 8000000) { showToast('工作流文件超过 8 MB', 'error'); return; }
        const fr = new FileReader();
        fr.onload = () => {
          if (destinationCanvas !== library.active) { showToast('已切换画布，未导入工作流'); return; }
          if (!restoreSnapshot(fr.result)) return;
          pushHistory();
          fitToView(true);
          showToast('已导入工作流');
        };
        fr.readAsText(f);
      });

      /* ========================================================================
         18. 顶部模式按钮与底部悬浮坞绑定
         ======================================================================== */
      document.getElementById('btn-studio-history').addEventListener('click', () => {
        saveNow();
        if (window.ipc) post({kind:'studio',canvasId:library.active}); else showToast('工作室记录在 Muse 桌面客户端中打开');
      });

      document.getElementById('btn-add-text').addEventListener('click', () => setSelection([createNode({ type: 'text' }).id]));
      document.getElementById('btn-add-image').addEventListener('click', () => setSelection([createNode({ type: 'image' }).id]));
      document.getElementById('btn-add-video').addEventListener('click', () => setSelection([createNode({ type: 'video' }).id]));
      btnUndo.addEventListener('click', undo);
      btnRedo.addEventListener('click', redo);
      document.getElementById('btn-fit-view').addEventListener('click', () => fitToView(false));
      document.getElementById('btn-zoom-out').addEventListener('click', () => zoomAt(state.viewport.scale * 0.85, window.innerWidth / 2, window.innerHeight / 2));
      document.getElementById('btn-zoom-in').addEventListener('click', () => zoomAt(state.viewport.scale * 1.15, window.innerWidth / 2, window.innerHeight / 2));
      zoomResetBtn.addEventListener('click', () => zoomAt(1, window.innerWidth / 2, window.innerHeight / 2));

      window.addEventListener('resize', () => {

        updateFloatingUI();
        drawMinimap();
      });

      /* ========================================================================
         19. 初始化：预置 4 节点电影级工作流并默认选中图片节点
         ======================================================================== */
      function initShowcase() {
        const scenePrompt = '索尼 A7S3，35mm f/1.4 电影定焦，雨后霓虹街道低照度写实摄影，冷蓝与琥珀暖光强烈对比，湿润路面倒影，细腻胶片颗粒';
        const motionPrompt = '极缓慢平滑推镜（Slow Push-In），焦点从前景雨滴过渡至中景建筑霓虹光影，空气中漂浮微尘粒子，24fps 电影动态模糊';

        const nScene = createNode({
          type: 'text',
          title: '画面提示词',
          x: 60,
          y: 120,
          content: scenePrompt,
        }, false);

        const preRenderedImg = '';
        const nImage = createNode({
          type: 'image',
          title: '图片',
          x: 430,
          y: 100,
          accountId: state.accounts[0]?.id || 'acc_pro',
          aspectRatio: '16:9',
          responseFormat: 'url',
          cameraTags: ['35mm f/1.4 低光'],
          outputUrl: preRenderedImg,
          lastMeta: '待生成 · 16:9',
        }, false);

        const nMotion = createNode({
          type: 'text',
          title: '运镜提示词',
          x: 60,
          y: 390,
          content: motionPrompt,
        }, false);

        const nVideo = createNode({
          type: 'video',
          title: '视频',
          x: 910,
          y: 180,
          accountId: state.accounts[0]?.id || 'acc_pro',
          aspectRatio: '16:9',
          duration: 6,
          outputUrl: '',
          isMockVideo: false,
          mockCoverUrl: preRenderedImg,
          lastMeta: '待生成 · 6s',
        }, false);

        state.connections = [
          { id: genId('e'), from: nScene.id, to: nImage.id },
          { id: genId('e'), from: nImage.id, to: nVideo.id },
          { id: genId('e'), from: nMotion.id, to: nVideo.id },
        ];

        refreshNodeContent(nImage);
        refreshNodeContent(nVideo);
        renderEdges();
        pushHistory();

        requestAnimationFrame(() => {
          fitToView(true);
          setSelection([nImage.id]);
        });
      }

      window.addEventListener('load', initializeDesktop);

      // Desktop integration: only explicit user generation reaches account pages.
      let saveTimer = null;
      let initialized = false;
      const library = {active:null, canvases:[], busy:false};
      const canvasHistories = new Map();
      const previewKey = 'muse_desktop_canvas_library_v1';
      const taskKey = (canvas, node) => `${canvas}:${node}`;
      const currentCanvas = () => library.canvases.find(c=>c.id===library.active);
      let importingFiles = 0;
      const pendingAssets = new Map();
      const post = message => window.ipc?.postMessage(JSON.stringify(message));
      const saveLabel = text => { document.getElementById('save-state').textContent = text; };

      function scheduleSave() {
        if (!initialized || library.busy) return;
        document.getElementById('canvas-empty').hidden = state.nodes.length !== 0;
        clearTimeout(saveTimer);
        saveLabel('正在保存…');
        saveTimer = setTimeout(saveNow, 250);
      }
      function saveNow() {
        if (!initialized || library.busy) return;
        clearTimeout(saveTimer);
        const graph = JSON.parse(serializeGraph());
        if (window.ipc) post({kind:'save', canvasId:library.active, graph});
        else {
          try { const saved=readPreviewLibrary(); saved.graphs[library.active]=graph; saved.active=library.active; localStorage.setItem(previewKey,JSON.stringify(saved)); saveLabel('预览 · 已保存'); }
          catch { saveLabel('预览 · 保存失败'); }
        }
      }
      window.addEventListener('beforeunload', saveNow);
      document.addEventListener('visibilitychange', () => { if(document.hidden) saveNow(); });

      function safeMedia(value) {
        if (typeof value !== 'string') return '';
        if (/^http:\/\/muse-canvas\.localhost\/media\/[a-zA-Z0-9_.-]+$/.test(value)) return value;
        if (/^data:(image\/(png|jpeg|webp|gif)|video\/(mp4|webm));base64,[a-zA-Z0-9+/=]+$/.test(value)) return value;
        return '';
      }
      function validateGraph(data) {
        if (!data || !Array.isArray(data.nodes) || !Array.isArray(data.connections) || data.nodes.length > 500 || data.connections.length > 2000) throw Error('graph');
        const ids = new Set();
        const nodes = data.nodes.map((n, i) => {
          if (!n || !['text','image','video'].includes(n.type) || typeof n.id !== 'string' || !/^[a-zA-Z][a-zA-Z0-9_-]{0,120}$/.test(n.id) || ids.has(n.id) || document.getElementById(n.id) && !document.getElementById(n.id).classList.contains('canvas-node')) throw Error('node');
          ids.add(n.id);
          const text = value => typeof value === 'string' ? value.slice(0, 50000) : '';
          const coordinate = value => Number.isFinite(value) ? clamp(value, -100000, 100000) : 0;
          return {id:n.id, type:n.type, title:text(n.title).slice(0,120), x:coordinate(n.x), y:coordinate(n.y), z:i+10,
            width:272, content:text(n.content), prompt:text(n.prompt), accountId:text(n.accountId),
            aspectRatio:(n.type==='video' ? VIDEO_RATIOS : IMAGE_RATIOS).some(r=>r.value===n.aspectRatio) ? n.aspectRatio : '16:9',
            duration:VIDEO_DURATIONS.includes(n.duration) ? n.duration : 6, responseFormat:'url',
            cameraTags:Array.isArray(n.cameraTags) ? n.cameraTags.filter(t=>CAMERA_TAGS.includes(t)) : [],
            outputUrl:safeMedia(n.outputUrl), localRefs:Array.isArray(n.localRefs) ? n.localRefs.slice(0,20).map(r=>({id:/^r_[a-zA-Z0-9_-]+$/.test(r?.id)?r.id:genId('r'),name:text(r?.name),url:safeMedia(r?.url)})).filter(r=>r.url) : [],
            requestId:text(n.requestId), isGenerating:false, isMockVideo:false, error:'', elapsed:0, lastMeta:text(n.lastMeta)};
        });
        const edges = new Set();
        const connections = data.connections.filter(c=>c && ids.has(c.from) && ids.has(c.to) && c.from!==c.to).filter(c=>{
          const key=c.from+'>'+c.to; if(edges.has(key)) return false; edges.add(key);
          const source=nodes.find(n=>n.id===c.from), target=nodes.find(n=>n.id===c.to);
          return source.type !== 'video' && target.type !== 'text';
        }).map(c=>({id:/^e_[a-zA-Z0-9_-]+$/.test(c.id)?c.id:genId('e'),from:c.from,to:c.to}));
        return {nodes, connections};
      }

      async function persistFile(file) {
        importingFiles++;
        try {
        if (!file || file.size > 32*1024*1024) throw Error('文件超过 32 MB，请选择较小的媒体文件');
        const data = await new Promise((resolve,reject)=>{
          const reader=new FileReader(); reader.onload=()=>resolve(reader.result);
          reader.onerror=()=>reject(Error('读取文件失败')); reader.readAsDataURL(file);
        });
        if (!safeMedia(data)) throw Error('支持 PNG、JPG、WebP、GIF、MP4 或 WebM');
        if (!window.ipc) return data;
        return await new Promise((resolve,reject)=>{
          const requestId=genId('asset');
          const timer=setTimeout(()=>{pendingAssets.delete(requestId);reject(Error('本地媒体保存超时'));},30000);
          pendingAssets.set(requestId,{resolve,reject,timer});
          post({kind:'import',requestId,data});
        });
        } finally { importingFiles--; }
      }

      function runDesktopGeneration(node) {
        if (!window.ipc) { node.error='请在 Muse 桌面客户端中启动并登录账号后生成'; updateFloatingUI(); return; }
        const account=state.accounts.find(a=>a.id===node.accountId);
        const prompt=getEffectivePrompt(node), references=getAllRefs(node).map(r=>r.url);
        if (!account || !account.running || account.busy) { node.error='请先启动并登录一个空闲账号，并在此节点中选择该账号'; updateFloatingUI(); return; }
        if (!prompt) { node.error='请输入提示词，或连接上游文本节点'; updateFloatingUI(); return; }
        if (references.length>1) { node.error='桌面账号当前支持一张参考图，请只保留一张参考图或首帧'; updateFloatingUI(); return; }
        if (getUpstream(node.id).images.some(image=>!image.url)) { node.error='上游图片尚未生成，请先生成或上传首帧图片'; updateFloatingUI(); return; }
        const requestId=genId('job');
        activeTasks.set(taskKey(library.active,node.id),{canvasId:library.active,nodeId:node.id,requestId});
        node.requestId=requestId;
        node.isGenerating=true; node.error=''; node.elapsed=0; node.genStage='正在连接账号…';
        refreshNodeContent(node); renderEdges(); updateFloatingUI();
        post({kind:'generate',canvasId:library.active,graph:JSON.parse(serializeGraph()),nodeId:node.id,requestId,accountId:node.accountId,mode:node.type,
          prompt,ratio:node.aspectRatio,duration:node.duration||6,references});
      }

      window.museCanvasDesktop = { receive(message) {
        if(message.kind==='theme') {
          applyTheme(message.theme);
        } else if(message.kind==='canvasLoaded') {
          applyCanvasLoaded(message);
        } else if(message.kind==='canvasError') {
          setLibraryBusy(false); saveLabel('操作未完成'); showToast(message.error,'error');
        } else if(message.kind==='accounts') {
          state.accounts=Array.isArray(message.accounts)?message.accounts:[];
          // Only assign unbound fresh nodes; never silently switch a chosen account.
          for(const node of state.nodes) if(!node.accountId || node.accountId==='acc_pro') node.accountId=state.accounts[0]?.id||'';
          updateFloatingUI(); renderAccountsList();
        } else if(message.kind==='saved') {
          if(message.canvasId!==library.active)return;
          saveLabel(message.error?'保存失败':'已保存到本机');
          if(message.error) showToast(message.error,'error');
        } else if(message.kind==='asset') {
          const pending=pendingAssets.get(message.requestId); if(!pending)return;
          clearTimeout(pending.timer); pendingAssets.delete(message.requestId);
          if(message.error)pending.reject(Error(message.error));else pending.resolve(message.url);
        } else if(message.kind==='result') {
          const key=taskKey(message.canvasId,message.nodeId);
          const task=activeTasks.get(key);
          if(!task || task.requestId!==message.requestId)return;
          if(message.canvasId!==library.active) {
            if(!message.running) {activeTasks.delete(key);canvasHistories.delete(message.canvasId);}
            renderCanvasLibrary();
            if(!message.running) showToast(`${library.canvases.find(c=>c.id===message.canvasId)?.name||'画布'}：${message.error||'生成完成'}`,message.error?'error':'success');
            return;
          }
          const node=state.nodes.find(n=>n.id===message.nodeId);
          if(!node || node.requestId!==message.requestId)return;
          node.isGenerating=Boolean(message.running); node.genStage=message.status||'';
          node.elapsed=message.elapsed||0; node.taskId=message.jobId||''; node.error=message.error||'';
          if(message.url)node.outputUrl=safeMedia(message.url);
          refreshNodeContent(node); renderEdges(); updateFloatingUI();
          if(!message.running) {
            activeTasks.delete(key); node.lastMeta=message.status||'';
            for(const edge of state.connections.filter(c=>c.from===node.id)) {
              const target=state.nodes.find(n=>n.id===edge.to);if(target)refreshNodeContent(target);
            }
            pushHistory(); drawMinimap(); showToast(message.error||'生成完成，已保存到本机',message.error?'error':'success');
          }
          renderCanvasLibrary();
          if(message.warning)showToast(message.warning,'error');
        }
      }};

      function setLibraryBusy(busy) {
        library.busy=busy;
        document.body.classList.toggle('is-board-loading',busy);
        document.getElementById('btn-canvas-library').disabled=busy;
      }

      function renderCanvasLibrary() {
        const current=currentCanvas();
        document.getElementById('canvas-current-name').textContent=current?.name||'我的画布';
        document.getElementById('canvas-count').textContent=`· ${library.canvases.length}`;
        document.title=`${current?.name||'我的画布'} · Muse Canvas`;
        const list=document.getElementById('canvas-library-list');
        list.replaceChildren();
        for(const canvas of library.canvases) {
          const row=document.createElement('button');
          row.type='button'; row.className=`canvas-library-row ${canvas.id===library.active?'is-active':''}`;
          row.dataset.canvasId=canvas.id;
          row.setAttribute('aria-current',canvas.id===library.active?'true':'false');
          const name=document.createElement('span');name.className='canvas-name';name.textContent=canvas.name;row.title=canvas.name;
          const detail=document.createElement('small');
          const running=[...activeTasks.values()].filter(task=>task.canvasId===canvas.id).length;
          detail.textContent=running?`${running} 项生成中`:canvas.id===library.active?'当前':'';
          row.append(name,detail);
          row.addEventListener('click',()=>{if(canvas.id!==library.active)canvasAction('select',{targetId:canvas.id});else closeCanvasLibrary();});
          list.appendChild(row);
        }
        const busy=[...activeTasks.values()].some(task=>task.canvasId===library.active);
        document.getElementById('btn-delete-canvas').disabled=busy;
        document.getElementById('btn-delete-canvas').title=busy?'请先完成或停止此画布的生成任务':'删除当前画布';
      }

      function closeCanvasLibrary() {
        document.getElementById('canvas-library').hidden=true;
        document.getElementById('btn-canvas-library').setAttribute('aria-expanded','false');
        document.getElementById('canvas-name-form').hidden=true;
        document.getElementById('canvas-delete-confirm').hidden=true;
      }
      function showCanvasNameForm(action) {
        const form=document.getElementById('canvas-name-form');
        form.dataset.action=action;form.hidden=false;
        document.getElementById('canvas-delete-confirm').hidden=true;
        document.getElementById('canvas-name-label').textContent=action==='create'?'新画布名称':'重命名当前画布';
        document.getElementById('btn-save-canvas-name').textContent=action==='create'?'创建画布':'保存名称';
        const input=document.getElementById('canvas-name-input');
        input.value=action==='create'?`未命名画布 ${library.canvases.length+1}`:currentCanvas()?.name||'';
        input.focus();input.select();
      }
      document.getElementById('btn-canvas-library').addEventListener('click',e=>{
        e.stopPropagation();if(!initialized)return;
        const panel=document.getElementById('canvas-library');
        if(!panel.hidden){closeCanvasLibrary();return;}
        closePopovers();renderCanvasLibrary();panel.hidden=false;
        e.currentTarget.setAttribute('aria-expanded','true');
      });
      document.getElementById('btn-new-canvas').addEventListener('click',()=>showCanvasNameForm('create'));
      document.getElementById('btn-rename-canvas').addEventListener('click',()=>showCanvasNameForm('rename'));
      document.getElementById('btn-cancel-canvas-name').addEventListener('click',()=>{document.getElementById('canvas-name-form').hidden=true;});
      document.getElementById('canvas-name-form').addEventListener('submit',e=>{
        e.preventDefault();const name=document.getElementById('canvas-name-input').value.trim();
        if(!name||[...name].length>80||/[\u0000-\u001f\u007f]/.test(name)){showToast('请输入 1 至 80 个字符的画布名称','error');return;}
        canvasAction(e.currentTarget.dataset.action,{name});
      });
      document.getElementById('btn-delete-canvas').addEventListener('click',()=>{
        document.getElementById('canvas-name-form').hidden=true;
        document.getElementById('canvas-delete-message').textContent=`删除「${currentCanvas()?.name}」及其节点和连线？已生成的媒体与工作室记录会保留。`;
        document.getElementById('canvas-delete-confirm').hidden=false;
      });
      document.getElementById('btn-cancel-delete-canvas').addEventListener('click',()=>{document.getElementById('canvas-delete-confirm').hidden=true;});
      document.getElementById('btn-confirm-delete-canvas').addEventListener('click',()=>canvasAction('delete'));
      document.addEventListener('pointerdown',e=>{if(!e.target.closest('#canvas-library,#btn-canvas-library'))closeCanvasLibrary();});
      document.addEventListener('keydown',e=>{if(e.key==='Escape')closeCanvasLibrary();});

      function readPreviewLibrary() {
        const saved=JSON.parse(localStorage.getItem(previewKey)||'null');
        if(!saved||saved.version!==1||!Array.isArray(saved.canvases)||!saved.graphs)throw Error('画布列表无法读取');
        return saved;
      }
      function canvasAction(action,extra={}) {
        if(!initialized||library.busy)return;
        if(importingFiles||pendingAssets.size){showToast('媒体正在保存，请稍后切换画布');return;}
        clearTimeout(saveTimer);
        const graph=JSON.parse(serializeGraph());
        setLibraryBusy(true);
        if(window.ipc) {post({kind:'canvasAction',action,canvasId:library.active,graph,...extra});return;}
        try {
          const saved=readPreviewLibrary();
          saved.graphs[library.active]=graph;
          if(action==='create') {
            if(saved.canvases.length>=500)throw Error('画布数量已达上限');
            const id=crypto.randomUUID();saved.canvases.push({id,name:extra.name});
            saved.graphs[id]={nodes:[],connections:[]};saved.active=id;
          } else if(action==='select') {
            if(!saved.canvases.some(c=>c.id===extra.targetId))throw Error('画布不存在');
            validateGraph(saved.graphs[extra.targetId]);saved.active=extra.targetId;
          } else if(action==='rename') {
            saved.canvases.find(c=>c.id===library.active).name=extra.name;
          } else if(action==='delete') {
            if([...activeTasks.values()].some(t=>t.canvasId===library.active))throw Error('请先完成或停止生成任务');
            saved.canvases=saved.canvases.filter(c=>c.id!==library.active);delete saved.graphs[library.active];
            if(!saved.canvases.length){const id=crypto.randomUUID();saved.canvases.push({id,name:'未命名画布'});saved.graphs[id]={nodes:[],connections:[]};}
            saved.active=saved.canvases[0].id;
          }
          localStorage.setItem(previewKey,JSON.stringify(saved));
          applyCanvasLoaded({library:saved,canvasId:saved.active,graph:saved.graphs[saved.active]});
        } catch(error) {setLibraryBusy(false);showToast(error.message,'error');}
      }

      function applyCanvasLoaded(message) {
        try {
          const graph=validateGraph(message.graph);
          if(!message.library?.canvases?.some(c=>c.id===message.canvasId))throw Error('画布列表无效');
          const previous=library.active;
          if(initialized && previous!==message.canvasId) {
            // Include the latest edits even when an input has not blurred yet.
            const snap=historySnapshot();
            if(state.history[state.historyIndex]!==snap) {
              state.history=state.history.slice(0,state.historyIndex+1);state.history.push(snap);state.historyIndex=state.history.length-1;
            }
            canvasHistories.set(previous,{history:state.history.slice(-50),index:Math.min(state.historyIndex,49)});
          }
          library.active=message.canvasId;library.canvases=message.library.canvases;
          if(Array.isArray(message.tasks)) {
            activeTasks.clear();
            for(const task of message.tasks)activeTasks.set(taskKey(task.canvasId,task.nodeId),task);
          }
          const switching=!initialized||previous!==library.active;
          if(switching) {
            initialized=false;closePopovers();state.selectedIds.clear();
            state.nodes=graph.nodes;state.connections=graph.connections;state.nextZ=Math.max(10,...state.nodes.map(n=>n.z||10));
            nodesLayer.replaceChildren();
            state.nodes.forEach(node=>{
              const task=activeTasks.get(taskKey(library.active,node.id));
              if(task?.requestId===node.requestId){node.isGenerating=true;node.genStage='生成任务进行中…';}
              if(!node.accountId||node.accountId==='acc_pro')node.accountId=state.accounts[0]?.id||'';
              renderNode(node);
            });
            const view=message.graph.viewport;
            if(view&&Number.isFinite(view.x)&&Number.isFinite(view.y)&&Number.isFinite(view.scale)) {
              state.viewport={x:clamp(view.x,-1000000,1000000),y:clamp(view.y,-1000000,1000000),scale:clamp(view.scale,state.minScale,state.maxScale)};applyViewport();
            } else fitToView(true);
            const previousHistory=canvasHistories.get(library.active);
            state.history=previousHistory?.history||[historySnapshot()];state.historyIndex=previousHistory?.index??0;
            renderEdges();updateFloatingUI();drawMinimap();updateHistoryButtons();
          }
          for(const id of canvasHistories.keys())if(!library.canvases.some(c=>c.id===id))canvasHistories.delete(id);
          initialized=true;setLibraryBusy(false);renderCanvasLibrary();closeCanvasLibrary();
          document.getElementById('canvas-empty').hidden=state.nodes.length!==0;
          saveLabel(window.ipc?'已保存到本机':'预览 · 已保存');
          if(message.error)showToast(message.error,'error');
        } catch(error) {setLibraryBusy(false);saveLabel('读取失败');showToast(error.message||'画布读取失败，未替换当前内容','error');}
      }

      function applyTheme(theme) {
        if(theme!=='light'&&theme!=='dark')return;
        document.documentElement.dataset.theme=theme;
        drawMinimap();
      }
      const systemTheme = window.matchMedia('(prefers-color-scheme: dark)');
      systemTheme.addEventListener('change',event=>{
        // In the desktop app, its resolved theme always wins over the OS.
        if(!window.ipc)applyTheme(event.matches?'dark':'light');
      });

      async function initializeDesktop() {
        applyTheme(document.documentElement.dataset.theme||(systemTheme.matches?'dark':'light'));
        if(window.ipc) { setLibraryBusy(true); post({kind:'ready'}); return; }
        try {
          let saved=JSON.parse(localStorage.getItem(previewKey)||'null');
          if(!saved) {
            const legacy=JSON.parse(localStorage.getItem('muse_desktop_canvas_preview')||'null');
            const id=crypto.randomUUID();
            saved={version:1,active:id,canvases:[{id,name:legacy?'我的画布':'未命名画布'}],graphs:{[id]:legacy||{nodes:[],connections:[]}}};
            localStorage.setItem(previewKey,JSON.stringify(saved));
          }
          applyCanvasLoaded({library:saved,canvasId:saved.active,graph:saved.graphs[saved.active]});
        } catch { setLibraryBusy(false); saveLabel('读取失败'); showToast('无法读取画布列表，原数据已保留','error'); }
      }

    })();
