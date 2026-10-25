from PIL import Image
import numpy as np, sys
src=Image.open(sys.argv[1]).convert('RGB')
im=np.array(src).astype(float)
bg=np.array([37,40,45.])
d=np.sqrt(((im-bg)**2).sum(-1))
a=np.clip((d-10)/22,0,1)
col=np.clip((im-(1-a[...,None])*bg)/np.maximum(a[...,None],1e-3),0,255)
R,G,B=col[...,0],col[...,1],col[...,2]
lum=0.3*R+0.6*G+0.1*B
flame=(R>170)&(R-B>70)&(a>0.5)
def sh(m,dy,dx): return np.roll(np.roll(m,dy,0),dx,1)
def dil(m,n):
    for _ in range(n): m=m|sh(m,1,0)|sh(m,-1,0)|sh(m,0,1)|sh(m,0,-1)
    return m
body=(a>0.9)&~flame&(lum>45)&~dil(flame,10)
lab=np.zeros(a.shape,np.int8); lab[flame]=1; lab[body]=2
for it in range(60):
    todo=(a>0)&(lab==0)
    if not todo.any(): break
    for L in (1,2):
        m=(lab==L); g=sh(m,1,0)|sh(m,-1,0)|sh(m,0,1)|sh(m,0,-1)
        lab[g&todo&(lab==0)]=L
box=(36,28,690,924)
for L,name in [(1,'llama'),(2,'lenos')]:
    alpha=(a*(lab==L)*255).astype(np.uint8)
    img=Image.fromarray(np.dstack([col.astype(np.uint8),alpha]),'RGBA').crop(box); img.save(name+'_c.png')
    c=Image.new('RGBA',img.size,(255,255,255,255)); c.alpha_composite(img); c.convert('RGB').resize((img.width//3,img.height//3)).save(name+'_preview.png')
both=Image.new('RGBA',Image.open('llama_c.png').size,(10,6,8,255)); both.alpha_composite(Image.open('lenos_c.png')); both.alpha_composite(Image.open('llama_c.png')); both.convert('RGB').resize((both.width//2,both.height//2)).save('junto_preview.png')
