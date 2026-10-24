// Рисует спектр на холсте. Движок отправляет около 30 картинок в секунду только тогда, когда открыт
// полноэкранный режим; между ними удерживается последний, а при остановке воспроизведения такты
// опускаются сами.

export const MODES = ["bars", "mirror", "wave", "radial", "off"];

const SINK_PER_FRAME = 0.9;

/** `#rrggbb` to `r, g, b`. */
function channels(hex) {
  const value = /^#[0-9a-f]{6}$/i.test(hex) ? hex : "#3385fa";
  return [1, 3, 5].map((i) => parseInt(value.slice(i, i + 2), 16));
}

/** Установите каждый радиальный штрих внутрь зарезервированной части крышки, включая ее колпачок. */
export function radialGeometry(spot, width, height, count) {
  if (!spot || !Number.isFinite(spot.radius) || spot.radius <= 0 || count <= 0) return null;
  const b=spot.bounds ?? {left:0,top:0,right:width,bottom:height};
  const outer=Math.min(spot.x-b.left,b.right-spot.x,spot.y-b.top,b.bottom-spot.y)-4;
  const line=Math.max(1.5,Math.min(3,spot.radius*0.012));
  const inner=spot.radius+Math.max(7,spot.radius*0.045);
  const reach=Math.min(48,spot.radius*0.24,outer-inner-line/2);
  if (reach < 3) return null;
  return {inner,reach,line,outer};
}

export function createVisualizer(canvas) {
  const context = canvas.getContext("2d");
  let mode = "bars";
  let colors = ["#3385fa", "#1a6be0"];
  let bands = [];
  let target = [];
  let anchor = () => null; // центр и размер крышки, для радиального режима
  let running = false;
  let frame = 0;
  let width = 0;
  let height = 0;
  let ratio = 1;
  let spotCache = null;
  let plotCache = null;
  let plotBounds = () => null;
  const phone = () => document.documentElement.dataset.layout === "phone";

  function resize() {
    spotCache = null;
    plotCache = null;
    ratio = Math.min(window.devicePixelRatio || 1, phone() ? 1.25 : 2);
    width = canvas.clientWidth;
    height = canvas.clientHeight;
    canvas.width = Math.max(1, Math.round(width * ratio));
    canvas.height = Math.max(1, Math.round(height * ratio));
    context.setTransform(ratio, 0, 0, ratio, 0, 0);
    paint();
  }

  function fill(x0, y0, x1, y1) {
    const gradient = context.createLinearGradient(x0, y0, x1, y1);
    gradient.addColorStop(0, `rgb(${channels(colors[0]).join(",")})`);
    gradient.addColorStop(1, `rgb(${channels(colors[1]).join(",")})`);
    return gradient;
  }

  function roundedBar(x, y, w, h) {
    const r = Math.min(w / 2, Math.abs(h) / 2);
    context.beginPath();
    context.roundRect(x, h < 0 ? y + h : y, w, Math.abs(h), r);
    context.fill();
  }

  function paint() {
    context.clearRect(0, 0, width, height);
    if (mode === "off" || !bands.length || !width || !height) return;
    const count = bands.length;
    const plot = phone() && mode !== "radial" ? (plotCache ??= plotBounds()) : null;
    if (plot && plot.height < 4) return;
    const plotHeight = plot?.height ?? height;
    const plotTop = plot?.top ?? 0;
    const plotLeft = plot?.left ?? 0;
    const plotWidth = plot?.width ?? width;
    const floor = plotHeight * 0.04;
    if (mode === "bars" || mode === "mirror") {
      const area = plotHeight * (plot ? 0.85 : 0.42);
      const step = plotWidth / count;
      const bar = Math.max(2, step * 0.62);
      const base = plot ? plotTop + plotHeight - 4 : height - 28;
      const baseline = mode === "bars" ? base : base - area / 2;
      context.fillStyle = fill(0, base - area, 0, base);
      for (let i = 0; i < count; i += 1) {
        const x = plotLeft + i * step + (step - bar) / 2;
        const level = bands[i] ** 1.4;
        if (mode === "bars") {
          roundedBar(x, baseline, bar, -(floor + level * area));
        } else {
          const half = (floor + level * area) / 2;
          roundedBar(x, baseline - half, bar, half * 2);
        }
      }
    } else if (mode === "wave") {
      const area = plotHeight * (plot ? 0.85 : 0.38);
      const baseline = plot ? plotTop + plotHeight / 2 : height - 28 - area / 2;
      const points = bands.map((level, i) => [plotLeft + (i / (count - 1)) * plotWidth, level ** 1.3 * area / 2 + floor / 2]);
      context.beginPath();
      context.moveTo(plotLeft, baseline);
      points.forEach(([x, y], i) => {
        const [px, py] = points[i - 1] ?? [x, y];
        context.quadraticCurveTo(px, baseline - py, (px + x) / 2, baseline - (py + y) / 2);
      });
      for (let i = points.length - 1; i >= 0; i -= 1) {
        const [x, y] = points[i];
        const [nx, ny] = points[i + 1] ?? [x, y];
        context.quadraticCurveTo(nx, baseline + ny, (nx + x) / 2, baseline + (ny + y) / 2);
      }
      context.closePath();
      context.globalAlpha = 0.85;
      context.fillStyle = fill(0, baseline - area / 2, 0, baseline + area / 2);
      context.fill();
      context.globalAlpha = 1;
    } else if (mode === "radial") {
      const spot = (spotCache ??= anchor()) ?? { x: width / 2, y: height / 2, radius: Math.min(width, height) * 0.2 };
      const total = count * 2; // зеркально отразил left/right, чтобы кольцо было симметричным
      const geometry=radialGeometry(spot,width,height,count);
      if (!geometry) return;
      const {inner,reach,line}=geometry;
      context.save();
      context.translate(spot.x, spot.y);
      context.lineCap = "round";
      context.lineWidth = line;
      context.globalAlpha = 0.55;
      context.strokeStyle = fill(-inner - reach, -inner - reach, inner + reach, inner + reach);
      for (let i = 0; i < total; i += 1) {
        const index = i < count ? i : total - 1 - i;
        const angle = (i / total) * Math.PI * 2 - Math.PI / 2;
        const length = 2 + bands[index] ** 1.4 * Math.max(0,reach-2);
        context.beginPath();
        context.moveTo(Math.cos(angle) * inner, Math.sin(angle) * inner);
        context.lineTo(Math.cos(angle) * (inner + length), Math.sin(angle) * (inner + length));
        context.stroke();
      }
      context.restore();
    }
  }

  function tick() {
    frame = 0;
    if (!running) return;
    let moving = false;
    // Без свежих данных (приостановлено, остановлено) столбцы обнуляются.
    bands = bands.map((value, i) => {
      const goal = target[i] ?? 0;
      if (value === goal) return value;
      moving = true;
      const next = goal > value ? goal : value * SINK_PER_FRAME;
      return next < 0.004 && goal === 0 ? 0 : Math.max(goal, next);
    });
    paint();
    if (moving) frame = requestAnimationFrame(tick);
  }

  const observer = new ResizeObserver(resize);
  observer.observe(canvas);

  return {
    redraw() { spotCache=null; plotCache=null; paint(); },
    get mode() { return mode; },
    setMode(next) { spotCache=null; plotCache=null; mode = MODES.includes(next) ? next : "bars"; paint(); },
    setColors(primary, secondary) { colors = [primary, secondary]; paint(); },
    setPlotBounds(fn) { plotBounds=fn; plotCache=null; },
    setAnchor(fn) { anchor = fn; spotCache=null; },
    /** Свежая картинка с движка: нарисовано сразу. */
    feed(levels) {
      if (!running || mode === "off") return;
      target = levels.slice(0,128).map(value => Number.isFinite(value) ? Math.max(0,Math.min(1,value)) : 0);
      bands = target.slice();
      // Coalesce phone events into a single paint before the next display frame.
      if (phone()) { if (!frame) frame=requestAnimationFrame(tick); }
      else paint();
    },
    /** Воспроизведение остановлено: пусть полоски осядут, а не замерзнут. */
    settle() {
      target = target.map(() => 0);
      if (running && !frame) frame = requestAnimationFrame(tick);
    },
    start() { running = true; resize(); },
    stop() {
      running = false;
      if (frame) cancelAnimationFrame(frame);
      frame = 0;
      bands = [];
      target = [];
      paint();
    },
    destroy() { observer.disconnect(); this.stop(); },
  };
}
