// Draws the spectrum on a canvas. The engine sends about 30 pictures a second
// only while the full-screen view is open; between them the last one is held,
// and when playback stops the bars sink on their own.

export const MODES = ["bars", "mirror", "wave", "radial", "off"];

const SINK_PER_FRAME = 0.9;

/** `#rrggbb` to `r, g, b`. */
function channels(hex) {
  const value = /^#[0-9a-f]{6}$/i.test(hex) ? hex : "#3385fa";
  return [1, 3, 5].map((i) => parseInt(value.slice(i, i + 2), 16));
}

/** Fit every radial stroke inside the cover's reserved stage, including its cap. */
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
  let anchor = () => null; // centre and size of the cover, for the radial mode
  let running = false;
  let frame = 0;
  let width = 0;
  let height = 0;
  let ratio = 1;

  function resize() {
    ratio = Math.min(window.devicePixelRatio || 1, 2);
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
    const floor = height * 0.04;
    if (mode === "bars" || mode === "mirror") {
      const area = height * 0.42;
      const step = width / count;
      const bar = Math.max(2, step * 0.62);
      const baseline = mode === "bars" ? height - 28 : height - 28 - area / 2;
      context.fillStyle = fill(0, height - 28 - area, 0, height - 28);
      for (let i = 0; i < count; i += 1) {
        const x = i * step + (step - bar) / 2;
        const level = bands[i] ** 1.4;
        if (mode === "bars") {
          roundedBar(x, baseline, bar, -(floor + level * area));
        } else {
          const half = (floor + level * area) / 2;
          roundedBar(x, baseline - half, bar, half * 2);
        }
      }
    } else if (mode === "wave") {
      const area = height * 0.38;
      const baseline = height - 28 - area / 2;
      const points = bands.map((level, i) => [(i / (count - 1)) * width, level ** 1.3 * area / 2 + floor / 2]);
      context.beginPath();
      context.moveTo(0, baseline);
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
      const spot = anchor() ?? { x: width / 2, y: height / 2, radius: Math.min(width, height) * 0.2 };
      const total = count * 2; // mirrored left/right so the ring is symmetric
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
    // Without fresh data (paused, stopped) the bars settle to zero.
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
    redraw() { paint(); },
    get mode() { return mode; },
    setMode(next) { mode = MODES.includes(next) ? next : "bars"; paint(); },
    setColors(primary, secondary) { colors = [primary, secondary]; paint(); },
    setAnchor(fn) { anchor = fn; },
    /** A fresh picture from the engine: drawn right away. */
    feed(levels) {
      target = levels.slice(0,128).map(value => Number.isFinite(value) ? Math.max(0,Math.min(1,value)) : 0);
      bands = target.slice();
      paint();
    },
    /** Playback stopped: let the bars sink instead of freezing. */
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
