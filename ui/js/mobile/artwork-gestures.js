import {isPhone,haptic} from './layout.js';

export function artworkGestureAction(translation,predicted) {
  const x=translation.x,y=translation.y;
  if(Math.abs(x)>=24 && Math.abs(x)>Math.abs(y)*1.25) {
    const flick=x*predicted.x>0 && Math.abs(predicted.x)>=90;
    if(Math.abs(x)>=58 || flick)return x<0?'next':'previous';
  }
  if(y<-52 && Math.abs(y)>Math.abs(x)*1.25)return 'queue';
  return null;
}
export function attachArtworkGestures(element,actions) {
  const controller=new AbortController(),options={signal:controller.signal};
  let drag=null;
  element.addEventListener('pointerdown',event=>{
    if(!isPhone() || (event.pointerType==='mouse' && event.button!==0))return;
    drag={id:event.pointerId,x:event.clientX,y:event.clientY,samples:[{x:event.clientX,y:event.clientY,t:performance.now()}]};
  },options);
  element.addEventListener('pointermove',event=>{
    if(!drag || drag.id!==event.pointerId)return;
    const now=performance.now();drag.samples.push({x:event.clientX,y:event.clientY,t:now});
    while(drag.samples.length>2 && now-drag.samples[0].t>100)drag.samples.shift();
    if(Math.abs(event.clientX-drag.x)>Math.abs(event.clientY-drag.y)*1.25 && Math.abs(event.clientX-drag.x)>20) {
      try{element.setPointerCapture(event.pointerId);}catch{/* view may have closed */}
    }
  },options);
  element.addEventListener('pointerup',event=>{
    if(!drag || drag.id!==event.pointerId)return;
    const current=drag;drag=null;
    const first=current.samples[0],last=current.samples.at(-1);
    const span=Math.max(1,last.t-first.t);
    const translation={x:event.clientX-current.x,y:event.clientY-current.y};
    const predicted={x:translation.x+(performance.now()-last.t<100?(last.x-first.x)/span*160:0),y:translation.y};
    const action=artworkGestureAction(translation,predicted);
    if(action){haptic(8);actions[action]?.();}
  },options);
  element.addEventListener('pointercancel',()=>{drag=null;},options);
  // Touch starts with implicit capture on the image. Moving capture to the
  // cover releases that child first; its bubbling event must not cancel us.
  element.addEventListener('lostpointercapture',event=>{if(event.target===element && drag?.id===event.pointerId)drag=null;},options);
  return ()=>{drag=null;controller.abort();};
}
