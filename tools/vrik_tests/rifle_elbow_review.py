"""Independently measure the visible right arm in the two captured stereo pairs."""
import argparse
import hashlib
import json
from pathlib import Path
import tkinter as tk

import numpy as np
from PIL import Image, ImageDraw, ImageFont, ImageTk

from rifle_elbow_live import measure_sample, save


def reconstruct(eyes, pixels):
    origins, rays = [], []
    for eye, (x,y) in zip(eyes,pixels):
        w,h = eye['color'][:2]
        l,r,u,d = np.tan(eye['fov'])
        if not (0<=x<w and 0<=y<h and l<r and d<u):
            raise ValueError('Invalid pixel or projection window')
        tx = l+(r-l)*(x+.5)/w
        ty = u+(d-u)*(y+.5)/h
        ray = np.array(eye['forward']) + np.array(eye['right'])*tx + np.array(eye['up'])*ty
        rays.append(ray/np.linalg.norm(ray))
        origins.append(np.array(eye['position']))
    if not .01<np.linalg.norm(origins[1]-origins[0])<.12:
        raise ValueError('Invalid stereo baseline')
    mat=np.column_stack((rays[0],-rays[1]))
    ts=np.linalg.lstsq(mat,origins[1]-origins[0],rcond=None)[0]
    if not np.all((.10<ts)&(ts<3.5)) or np.linalg.cond(mat)>180:
        raise ValueError('Unusable stereo disparity at the selected landmark')
    pair=[o+t*v for o,t,v in zip(origins,ts,rays)]
    point=(pair[0]+pair[1])*.5
    # Check in the source images, independent of resolution or distance from the camera.
    for eye,pixel in zip(eyes,pixels):
        rel=point-eye['position']
        z=rel@eye['forward']
        l,r,u,d=np.tan(eye['fov'])
        uv=np.array([(rel@eye['right']/z-l)/(r-l),(u-rel@eye['up']/z)/(u-d)])
        if np.linalg.norm(uv*eye['color'][:2]-.5-pixel)>4:
            raise ValueError('Stereo marks disagree by more than four source pixels')
    return point


def depth_patch(raw,eye,pixel):
    """Median forward depth over 3x3 source texels. Match stereo_check's minus-jitter convention."""
    w,h,fmt=eye['depth']
    if fmt not in (39,40,41) or raw.stat().st_size!=w*h*4:
        raise ValueError('Expected a packed 32-bit float depth image')
    cw,ch=eye['color'][:2]
    x,y=np.floor((np.array(pixel)+.5)*[w/cw,h/ch]+eye['jitter']).astype(int)
    if not (1<=x<w-1 and 1<=y<h-1):
        raise ValueError('Depth patch outside image')
    with raw.open('rb') as stream:
        rows=[]
        for row in range(y-1,y+2):
            stream.seek(4*(row*w+x-1))
            rows.extend(np.frombuffer(stream.read(12),dtype='<f4'))
    samples=np.array(rows,dtype=float)
    if not np.isfinite(samples).all() or np.any(samples<=1e-7):
        raise ValueError('Unmeasurable depth patch')
    near,far=eye['planes'][:2]
    if eye['infinite']:
        if not eye['inverted']: raise ValueError('Unsupported infinite forward depth')
        z=near/samples
    elif eye['inverted']:
        z=(near*far)/((far-near)*samples+near)
    else:
        z=(near*far)/(far-(far-near)*samples)
    return dict(texel=[int(x),int(y)],medianM=float(np.median(z)),rangeM=[float(z.min()),float(z.max())])


def review_marks(out, filename):
    """Replay inspected image coordinates, including explicitly obscured joints."""
    cap=json.loads((out/'capture.json').read_text())
    marks=json.loads(filename.read_text())
    if not cap['complete']:
        raise ValueError('Incomplete capture')
    report=dict(source='Visible stereo landmarks and recorded eye depth',phases={},reviewed=False,complete=False)
    for label in ('before','after'):
        pick=marks[label]
        phase=cap['phases'][label]
        frame=phase['frame']
        eyes=frame['eyes']
        if pick['frame']!=frame['frame']:
            raise ValueError('Marks belong to another frame')
        for side,digest in zip(('L','R'),pick['sha256'],strict=True):
            image=out/label/(side+'.jpg')
            if hashlib.sha256(image.read_bytes()).hexdigest()!=digest:
                raise ValueError('Marks belong to another eye image')
        result=dict(frame=frame['frame'],notes=pick['notes'],surface={},depth={},visibleAngles=None)
        for name,pair in pick['surface'].items():
            world=reconstruct(eyes,pair)
            result['surface'][name]=dict(pixels=pair,worldM=world.tolist(),
                forwardDepthM=[float((world-e['position'])@e['forward']) for e in eyes])
        for name,pair in marks['depthSightlines'].items():
            result['depth'][name]=[depth_patch(Path(phase['raw'])/f"f{phase['index']}_e{i}_depth.raw",eye,p)
                                   for i,(eye,p) in enumerate(zip(eyes,pair,strict=True))]
        result['joints']={n:reconstruct(eyes,p).tolist() if p is not None else None
                          for n,p in pick['joints'].items()}
        pts=[result['joints'][n] for n in ('shoulder','elbow','wrist')]
        if all(p is not None for p in pts):
            s,e,w=np.asarray(pts)
            model=np.array(eyes[0]['rightArmModel']).reshape(3,3)
            if np.max(np.abs(np.linalg.norm(np.diff(pts,axis=0),axis=1)-
                             np.linalg.norm(np.diff(model,axis=0),axis=1)))>.04:
                raise ValueError('Marked arm lengths disagree with the matched skeleton')
            result['visibleAngles']=measure_sample(s,w,e)
        report['phases'][label]=result
    report['reviewed']=True
    report['complete']=all(v['visibleAngles'] is not None for v in report['phases'].values())
    save(out/'visible-angles.json',report)
    if 'cropLR' in marks:
        save_contact_sheet(out,marks,report)
    return report


def save_contact_sheet(out,marks,report):
    crops=marks['cropLR']
    width=620
    height=round(width*(crops[0][3]-crops[0][1])/(crops[0][2]-crops[0][0]))
    sheet=Image.new('RGB',(2*width,52+2*(height+36)),'#172025')
    draw=ImageDraw.Draw(sheet)
    font=ImageFont.load_default(size=20)
    names=list(marks['depthSightlines'])
    colors=['#ffcb4a','#54e7e5']
    for i,(name,color) in enumerate(zip(names,colors,strict=True)):
        draw.text((15+i*width,12),f'{chr(65+i)}   {marks["legend"][name]}',fill=color,font=font)
    for row,phase in enumerate(('before','after')):
        for col,side in enumerate(('L','R')):
            box=crops[col]
            x0,y0=width*col,52+(height+36)*row
            draw.text((x0+12,y0+4),f'{phase.title()}   /   {side} eye   /   frame {report["phases"][phase]["frame"]}',
                      fill='white',font=font)
            with Image.open(out/phase/(side+'.jpg')) as im:
                sheet.paste(im.crop(box).resize((width,height),Image.Resampling.LANCZOS),(x0,y0+36))
            for i,name in enumerate(names):
                x,y=marks['depthSightlines'][name][col]
                x=x0+(x-box[0])*width/(box[2]-box[0])
                y=y0+36+(y-box[1])*height/(box[3]-box[1])
                draw.ellipse((x-7,y-7,x+7,y+7),outline=colors[i],width=2)
                draw.text((x+10,y-18),chr(65+i),font=font,fill=colors[i],stroke_width=2,stroke_fill='#182028')
    sheet.save(out/'visible-landmarks.jpg',quality=92)


def mark(path, label):
    with Image.open(path) as f:
        im=f.convert('RGB')
    win=tk.Tk()
    win.title(label)
    scale=min(1.,(win.winfo_screenwidth()-80)/im.width,(win.winfo_screenheight()-150)/im.height)
    display=im.resize((int(im.width*scale),int(im.height*scale)),Image.Resampling.LANCZOS)
    photo=ImageTk.PhotoImage(display)
    message=tk.StringVar()
    tk.Label(win,textvariable=message).pack()
    canvas=tk.Canvas(win,width=display.width,height=display.height,highlightthickness=0)
    canvas.pack()
    canvas.create_image(0,0,image=photo,anchor='nw')
    points=[]
    accepted=False
    def redraw():
        names=['right shoulder joint','right elbow joint','right wrist joint','Enter to accept']
        message.set(f'{label}   |   {names[len(points)]}   |   Backspace undoes, Esc skips an obscured arm')
        canvas.delete('marks')
        for x,y in points:
            a,b=x*display.width/im.width,y*display.height/im.height
            canvas.create_oval(a-3,b-3,a+3,b+3,outline='yellow',tags='marks')
    def click(event):
        if len(points)<3:
            points.append([event.x*im.width/display.width,event.y*im.height/display.height])
            redraw()
    def accept(_):
        nonlocal accepted
        if len(points)==3:
            accepted=True
            win.destroy()
    canvas.bind('<Button-1>',click)
    win.bind('<BackSpace>',lambda _: (points.pop(),redraw()) if points else None)
    win.bind('<Return>',accept)
    win.bind('<Escape>',lambda _:win.destroy())
    redraw()
    win.mainloop()
    if not accepted:
        raise ValueError('Incomplete visible landmarks')
    overlay=ImageDraw.Draw(im)
    overlay.line([tuple(p) for p in points],fill='yellow',width=3)
    for p,name in zip(points,('S','E','W')):
        overlay.text(tuple(p),name,fill='yellow',stroke_width=1,stroke_fill='black')
    im.save(path.with_stem(path.stem+'-marked'))
    return points


def review(out):
    cap=json.loads((out/'capture.json').read_text())
    if not cap['complete']:
        raise ValueError('Incomplete capture')
    report={'source':'Triangulated visible landmarks in the captured eye images','phases':{},'complete':False}
    try:
        for phase in ('before','after'):
            eyes=cap['phases'][phase]['frame']['eyes']
            marks=[]
            for eye,side in zip(eyes,('L','R')):
                path=out/phase/(side+'.jpg')
                with Image.open(path) as im:
                    if list(im.size)!=eye['color'][:2]:
                        raise ValueError('Image size differs from frame metadata')
                marks.append(mark(path,phase+' / '+side))
            s,e,w=[reconstruct(eyes,p) for p in zip(*marks)]
            lengths=[np.linalg.norm(s-e),np.linalg.norm(e-w)]
            model=np.array(eyes[0]['rightArmModel']).reshape(3,3)
            reference=np.linalg.norm(np.diff(model,axis=0),axis=1)
            if np.max(np.abs(reference-lengths))>.04:
                raise ValueError('Visible segment lengths differ from the matched skeleton by over 4 cm')
            report['phases'][phase]=dict(pixels=marks,world=np.array([s,e,w]).tolist(),
                angles=measure_sample(s,w,e),frame=cap['phases'][phase]['frame']['frame'])
        before,after=[v['angles'] for v in report['phases'].values()]
        report['afterMinusBefore']={k:after[k]-before[k] for k in before}
        report['complete']=True
    except BaseException as exc:
        report['error']=str(exc)
        raise
    finally:
        save(out/'visible-angles.json',report)
    print(json.dumps(report['phases'],indent=2))


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('out',type=Path)
    parser.add_argument('--marks',type=Path,help='Replay reviewed stereo picks and trace their saved depth')
    args=parser.parse_args()
    if args.marks:
        result=review_marks(args.out,args.marks)
        print('Wrote',args.out/'visible-angles.json','complete visible joint angles:',result['complete'])
    else:
        review(args.out)
