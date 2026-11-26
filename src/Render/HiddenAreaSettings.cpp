#include "Render/HiddenArea.hpp"
#include <algorithm>
#include <cmath>

#if !CVR_RUST_VRIK
// The C++ build's geometry; the Rust build's is rust/crates/cvr-render/src/hidden_area.rs (the header calls it).
namespace cvr::hidden_area {
Settings Normalize(Settings s) {
    s.enabled=s.enabled?1:0;
    s.radius=std::isfinite(s.radius)?std::clamp(s.radius,MinRadius,MaxRadius):Settings{}.radius;
    return s;
}
bool ParseSetting(const char* line,Settings& s) {
    if(sscanf_s(line,"xr_hidden_area_radius = %f",&s.radius)==1)return true;
    return sscanf_s(line,"xr_hidden_area = %d",&s.enabled)==1;
}
void WriteSettings(FILE* file,Settings s) {
    s=Normalize(s);
    std::fprintf(file,"xr_hidden_area=%d\nxr_hidden_area_radius=%.1f\n",s.enabled,s.radius);
}

bool Hidden(const Fov& f,float radiusPercent,float x,float y) {
    if(!(x>=f.left && x<=f.right && y>=f.down && y<=f.up))return true;
    // Normalised per quadrant, so an asymmetric panel gets an ellipse that touches each of its
    // own edges at 100%.
    const float extentX=x<0?-f.left:f.right, extentY=y<0?-f.down:f.up;
    if(!(extentX>0) || !(extentY>0))return true;
    const float nx=x/extentX, ny=y/extentY, r=radiusPercent*.01f;
    return nx*nx+ny*ny>r*r;
}

std::vector<Rect> BuildRects(const Fov& f,float radiusPercent,uint32_t w,uint32_t h,float tanX,float tanY,
                             uint32_t cell,uint32_t margin) {
    if(!(tanX>0) || !(tanY>0))return {};
    return BuildRects(f,radiusPercent,w,h,Fov{-tanX,tanX,tanY,-tanY},cell,margin);
}

bool ShaderEllipseScales(const Fov& f,float radiusPercent,const Fov& rendered,float scales[4]) {
    const float sx=rendered.right-rendered.left, sy=rendered.up-rendered.down;
    if(!(sx>0) || !(sy>0) || !scales)return false;
    // NDC (y up) -> tangent of the rendered window.
    const auto visible=[&](float nx,float ny){
        return !Hidden(f,radiusPercent,rendered.left+(nx+1)*.5f*sx,rendered.down+(ny+1)*.5f*sy);
    };
    if(!visible(0,0))return false;
    // Along each axis, the farthest visible point.
    constexpr int Steps=512;
    const float dir[4][2]{{-1,0},{1,0},{0,1},{0,-1}};
    for(int i=0;i<4;++i) {
        int last=0;
        for(int k=1;k<=Steps;++k)if(visible(dir[i][0]*k/Steps,dir[i][1]*k/Steps))last=k;
        const float extent=std::min(1.0f,float(last+1)/Steps);
        scales[i]=1/(extent*extent);
    }
    // Off-centre, the visible area is not an ellipse about NDC 0; grow the fit until it holds all of it.
    constexpr int Grid=160;
    float worst=1;
    for(int y=0;y<=Grid;++y)for(int x=0;x<=Grid;++x) {
        const float nx=2.0f*x/Grid-1, ny=2.0f*y/Grid-1;
        if(!visible(nx,ny))continue;
        // One grid cell of slack: the test above samples, the shader sees every pixel.
        const float ax=std::fabs(nx)+1.0f/Grid, ay=std::fabs(ny)+1.0f/Grid;
        worst=std::max(worst,scales[nx<0?0:1]*ax*ax+scales[ny>0?2:3]*ay*ay);
    }
    for(int i=0;i<4;++i)scales[i]/=worst;
    return true;
}

std::vector<Rect> BuildRects(const Fov& f,float radiusPercent,uint32_t w,uint32_t h,const Fov& rendered,
                             uint32_t cell,uint32_t margin) {
    const float spanX=rendered.right-rendered.left, spanY=rendered.up-rendered.down;
    if(!w || !h || w>16384 || h>16384 || !(spanX>0) || !(spanY>0) || !cell || cell>64 || margin>8)return {};
    const uint32_t columns=(w+cell-1)/cell, rows=(h+cell-1)/cell;
    // Pixel position -> tangent of the rendered window (y up).
    const auto hidden=[&](float px,float py){
        return Hidden(f,radiusPercent,rendered.left+px/w*spanX,rendered.up-py/h*spanY);
    };
    std::vector<uint8_t> full(size_t(columns)*rows);
    for(uint32_t r=0;r<rows;++r)for(uint32_t c=0;c<columns;++c) {
        const float x0=float(c*cell),y0=float(r*cell),x1=float(std::min(w,(c+1)*cell)),y1=float(std::min(h,(r+1)*cell));
        const float xm=(x0+x1)*.5f, ym=(y0+y1)*.5f;
        full[size_t(r)*columns+c]=hidden(x0,y0) && hidden(x1,y0) && hidden(x0,y1) && hidden(x1,y1) &&
            hidden(xm,y0) && hidden(xm,y1) && hidden(x0,ym) && hidden(x1,ym) && hidden(xm,ym);
    }
    // Keep a band of shaded cells around the visible area: filters that read neighbours (bloom,
    // AO blur, DLSS) then never reach a masked pixel from a visible one. Off-target counts as hidden.
    std::vector<uint8_t> masked(full.size());
    for(uint32_t r=0;r<rows;++r)for(uint32_t c=0;c<columns;++c) {
        bool keep=full[size_t(r)*columns+c]!=0;
        for(int dy=-int(margin);keep && dy<=int(margin);++dy)for(int dx=-int(margin);keep && dx<=int(margin);++dx) {
            const int rr=int(r)+dy, cc=int(c)+dx;
            if(rr<0 || cc<0 || rr>=int(rows) || cc>=int(columns))continue;
            keep=full[size_t(rr)*columns+cc]!=0;
        }
        masked[size_t(r)*columns+c]=keep;
    }
    std::vector<Rect> out;
    std::vector<std::pair<uint32_t,uint32_t>> previous,current;
    std::vector<size_t> previousIndex,currentIndex;
    for(uint32_t r=0;r<rows;++r) {
        current.clear();currentIndex.clear();
        for(uint32_t c=0;c<columns;) {
            if(!masked[size_t(r)*columns+c]){++c;continue;}
            uint32_t end=c;
            while(end<columns && masked[size_t(r)*columns+end])++end;
            current.emplace_back(c,end);
            c=end;
        }
        for(const auto& run:current) {
            const auto it=std::find(previous.begin(),previous.end(),run);
            if(it!=previous.end()) {
                const size_t index=previousIndex[size_t(it-previous.begin())];
                out[index].bottom=int32_t(std::min(h,(r+1)*cell));
                currentIndex.push_back(index);
            } else {
                out.push_back({int32_t(run.first*cell),int32_t(r*cell),int32_t(std::min(w,run.second*cell)),int32_t(std::min(h,(r+1)*cell))});
                currentIndex.push_back(out.size()-1);
            }
        }
        previous.swap(current);previousIndex.swap(currentIndex);
    }
    return out;
}

double Coverage(const std::vector<Rect>& rects,uint32_t w,uint32_t h) {
    if(!w || !h)return 0;
    double area=0;
    for(const auto& r:rects)area+=double(r.right-r.left)*double(r.bottom-r.top);
    return area/(double(w)*h);
}
}
#endif
