import type { CaptureTarget, CropRegion } from "../recorder/types";

export function normalizeRegionFromNativeSelection(
  selectedAbsolute: CropRegion,
  target: CaptureTarget,
): CropRegion {
  // La selección nativa llega en coordenadas absolutas de pantalla.
  // Convertir a coordenadas relativas del target y ajustar por escala DPI.
  const relX = selectedAbsolute.x - target.originX;
  const relY = selectedAbsolute.y - target.originY;

  const scaleX = target.width / Math.max(1, target.screenWidth);
  const scaleY = target.height / Math.max(1, target.screenHeight);

  const mappedX = Math.max(0, Math.min(target.width - 1, Math.round(relX * scaleX)));
  const mappedY = Math.max(0, Math.min(target.height - 1, Math.round(relY * scaleY)));
  const mappedWidth = Math.max(
    1,
    Math.min(target.width - mappedX, Math.round(selectedAbsolute.width * scaleX)),
  );
  const mappedHeight = Math.max(
    1,
    Math.min(target.height - mappedY, Math.round(selectedAbsolute.height * scaleY)),
  );

  return {
    x: mappedX,
    y: mappedY,
    width: mappedWidth,
    height: mappedHeight,
  };
}
