import React, { createContext, useContext, useEffect, useRef } from 'react';
import type { InteractiveCanvas } from 'csdm/ui/hooks/use-interactive-map-canvas';

const RadarContext = createContext<string | null>(null);

export function NativeRadarProvider({ src, children }: { src: string | null; children: React.ReactNode }) {
  return <RadarContext.Provider value={src}>{children}</RadarContext.Provider>;
}

// The pinned CSDM canvas and map transforms remain in use. Only its Electron
// file:// radar loader is replaced: selected artwork stays in WebView memory.
export function useDrawMapRadar() {
  const src = useContext(RadarContext);
  const image = useRef<HTMLImageElement | null>(null);
  useEffect(() => {
    image.current = null;
    if (!src) return;
    const loaded = new Image();
    loaded.onload = () => { image.current = loaded; };
    loaded.src = src;
    return () => { loaded.onload = null; image.current = null; };
  }, [src]);

  const drawMapRadar = (context: CanvasRenderingContext2D, canvas: InteractiveCanvas) => {
    const x = canvas.zoomedX(0);
    const y = canvas.zoomedY(0);
    const size = canvas.getScaledRadarSize();
    if (image.current) {
      context.drawImage(image.current, x, y, size, size);
      return;
    }
    context.fillStyle = '#1c2228';
    context.fillRect(x, y, size, size);
    context.strokeStyle = '#36404a';
    context.lineWidth = 1;
    for (let index = 0; index <= 16; index++) {
      const offset = size * index / 16;
      context.beginPath();
      context.moveTo(x + offset, y);
      context.lineTo(x + offset, y + size);
      context.moveTo(x, y + offset);
      context.lineTo(x + size, y + offset);
      context.stroke();
    }
  };
  return { drawMapRadar };
}
