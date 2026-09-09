"""Rasterize tessellated egui output for offline visual QA (Pillow + NumPy).
Run cargo test -p flvstx-plugin export_editor_preview -- --ignored first.
"""
import json
from pathlib import Path
import numpy as np
from PIL import Image

root = Path(__file__).resolve().parent.parent
data = json.loads((root / 'target/ui-preview.json').read_text())
w, h = data['size']
canvas = np.empty((h, w, 4), dtype=np.float32)
canvas[:] = [16, 21, 28, 255]
atlas = None
for delta in data['textures']:
    tw, th = delta['size']
    pixels = np.array(delta['pixels'], dtype=np.float32).reshape(th, tw, 4)
    if delta['pos'] is None:
        atlas = pixels
    else:
        x, y = delta['pos']
        atlas[y:y+th, x:x+tw] = pixels
ah, aw = atlas.shape[:2]
for mesh in data['meshes']:
    v = mesh['vertices']
    pos = np.array([x[:2] for x in v], dtype=np.float32)
    uv = np.array([x[2:4] for x in v], dtype=np.float32)
    color = np.array([x[4] for x in v], dtype=np.float32)
    cx0, cy0, cx1, cy1 = mesh['clip']
    for ids in np.array(mesh['indices']).reshape(-1, 3):
        tri = pos[ids]
        x0 = max(0, int(np.floor(tri[:, 0].min())), int(np.ceil(cx0)))
        y0 = max(0, int(np.floor(tri[:, 1].min())), int(np.ceil(cy0)))
        x1 = min(w, int(np.ceil(tri[:, 0].max())), int(np.ceil(cx1)))
        y1 = min(h, int(np.ceil(tri[:, 1].max())), int(np.ceil(cy1)))
        if x0 >= x1 or y0 >= y1: continue
        a, b, c = tri
        den = (b[1]-c[1])*(a[0]-c[0]) + (c[0]-b[0])*(a[1]-c[1])
        if abs(den) < 1e-8: continue
        yy, xx = np.mgrid[y0:y1, x0:x1].astype(np.float32)
        xx += .5; yy += .5
        wa = ((b[1]-c[1])*(xx-c[0])+(c[0]-b[0])*(yy-c[1]))/den
        wb = ((c[1]-a[1])*(xx-c[0])+(a[0]-c[0])*(yy-c[1]))/den
        wc = 1-wa-wb
        mask = (wa >= -1e-6) & (wb >= -1e-6) & (wc >= -1e-6)
        if not mask.any(): continue
        weights = np.stack([wa[mask], wb[mask], wc[mask]], axis=1)
        coords = weights @ uv[ids]
        tx = np.clip(coords[:, 0]*aw-.5, 0, aw-1)
        ty = np.clip(coords[:, 1]*ah-.5, 0, ah-1)
        ix = tx.astype(int); iy = ty.astype(int)
        jx = np.minimum(ix+1,aw-1); jy = np.minimum(iy+1,ah-1)
        fx=(tx-ix)[:,None]; fy=(ty-iy)[:,None]
        tex=(atlas[iy,ix]*(1-fx)+atlas[iy,jx]*fx)*(1-fy)+(atlas[jy,ix]*(1-fx)+atlas[jy,jx]*fx)*fy
        src = tex*(weights @ color[ids])/255
        dst = canvas[y0:y1,x0:x1]
        dst[mask] = src + dst[mask]*(1-src[:,3:4]/255)
path=root/'target/ui-preview.png'
Image.fromarray(np.clip(canvas[:,:,:3],0,255).astype('uint8')).save(path)
print(path)
