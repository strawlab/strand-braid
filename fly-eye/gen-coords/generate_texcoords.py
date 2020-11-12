import drosophila_eye_map.precomputed_buchner71 as optics
import drosophila_eye_map.util
import numpy as np

from mpl_toolkits.basemap import Basemap  # basemap > 0.9.9.1

# SMALL=True
SMALL = False

def myfloatstr(f):
    if SMALL:
        return '%.4f' % (f,)
    else:
        return repr(f)

def xyz2lonlat(x,y,z):
    lon_scale = -1.0

    lon1,lat = drosophila_eye_map.util.xyz2lonlat(x,y,z)
    lon1 = lon1 * lon_scale
    return lon1,lat


def calc_gain_offset(vals, min_val_out, max_val_out, flip_gain=False):
    min = np.min(vals)
    max = np.max(vals)
    diff_in = max-min
    diff_out = max_val_out-min_val_out

    gain = diff_out/diff_in
    if flip_gain:
        gain = -gain
        offset = -(min*gain) + max_val_out
    else:
        offset = -(min*gain) + min_val_out

    return gain, offset


if 1:
    proj='robin'
    kws = dict(resolution=None)
    if proj in ['moll','sinu','robin']:
            kws.update( dict(lon_0=0))

    basemap_instance = Basemap(projection=proj, **kws)
    all_rdirs = optics.receptor_dirs
    all_hex_faces = optics.hex_faces

    left_rdirs = all_rdirs[ optics.receptor_dir_slicer[ 'left' ] ]
    right_rdirs = all_rdirs[ optics.receptor_dir_slicer[ 'right' ] ]

    rdirs2 = [ xyz2lonlat( rdir.x, rdir.y, rdir.z ) for rdir in all_rdirs ]
    lons, lats = zip(*rdirs2)
    rdirs2_x, rdirs2_y = basemap_instance(lons, lats)

    rx_offset = 0.15
    y_margin = 0.01
    xgain, xoffset = calc_gain_offset(rdirs2_x, -1.0,            1.0 - rx_offset)
    ygain, yoffset = calc_gain_offset(rdirs2_y, -1.0 + y_margin, 1.0 - y_margin)

    all_hf2s = []
    for hf in all_hex_faces[optics.receptor_dir_slicer['left']]:
        hf2 = []
        for rdir in hf:
            hf2.append(xyz2lonlat(rdir.x, rdir.y, rdir.z))
        lons, lats = zip(*hf2)
        hf2_x, hf2_y = basemap_instance(lons, lats)
        all_hf2s.append((np.array(hf2_x)*xgain+xoffset,
                         np.array(hf2_y)*ygain+yoffset))
    for hf in all_hex_faces[optics.receptor_dir_slicer['right']]:
        hf2 = []
        for rdir in hf:
            hf2.append(xyz2lonlat(rdir.x, rdir.y, rdir.z))
        lons, lats = zip(*hf2)
        hf2_x, hf2_y = basemap_instance(lons, lats)
        all_hf2s.append((np.array(hf2_x)*xgain+xoffset+rx_offset,
                         np.array(hf2_y)*ygain+yoffset))

    left_faces = all_hf2s[optics.receptor_dir_slicer['left']]
    right_faces = all_hf2s[optics.receptor_dir_slicer['right']]

    txgain, txoffset = calc_gain_offset(rdirs2_x, 0, 0.98)
    tygain, tyoffset = calc_gain_offset(rdirs2_y, 0, 1.0, flip_gain=True)

    left_rdirs2 = [xyz2lonlat(rdir.x, rdir.y, rdir.z) for rdir in left_rdirs]
    lons, lats = zip(*left_rdirs2)
    left_rdirs2_x, left_rdirs2_y = basemap_instance(lons, lats)

    right_rdirs2 = [xyz2lonlat(rdir.x, rdir.y, rdir.z) for rdir in right_rdirs]
    lons, lats = zip(*right_rdirs2)
    right_rdirs2_x, right_rdirs2_y = basemap_instance(lons, lats)

    left_rdirs2_x = np.asarray(left_rdirs2_x)
    left_rdirs2_y = np.asarray(left_rdirs2_y)

    right_rdirs2_x = np.asarray(right_rdirs2_x)
    right_rdirs2_y = np.asarray(right_rdirs2_y)

    # vertex coords
    lx = left_rdirs2_x*xgain+xoffset
    ly = left_rdirs2_y*ygain+yoffset

    # texture coords
    ltx = left_rdirs2_x*txgain+txoffset
    lty = left_rdirs2_y*tygain+tyoffset

    # vertex coords
    rx = right_rdirs2_x*xgain+xoffset+rx_offset
    ry = right_rdirs2_y*ygain+yoffset

    # texture coords
    rtx = right_rdirs2_x*txgain+txoffset
    rty = right_rdirs2_y*tygain+tyoffset

    if 1:

        hex_verts_x = []
        hex_verts_y = []
        hex_verts_tx = []
        hex_verts_ty = []
        hex_idxs = []
        idx_offset = 0

        # left
        for face_enum,(x,y,face,tx,ty) in enumerate(zip(lx,ly,left_faces,ltx,lty)):
            face_x, face_y = face
            n = len(face_x)+1 # include center
            hex_verts_x.append( x ) # center
            hex_verts_y.append( y ) # center
            hex_verts_x.extend( list(face_x) )
            hex_verts_y.extend( list(face_y) )
            hex_verts_tx.extend( [tx]*n )
            hex_verts_ty.extend( [ty]*n )
            for i in range(2,len(face_x)+1):
                tri = [idx_offset,idx_offset+i-1,idx_offset+i]
                hex_idxs.append(tri)
            hex_idxs.append( [idx_offset,idx_offset+len(face_x), idx_offset+1] )
            idx_offset += n

        # right
        for face_enum,(x,y,face,tx,ty) in enumerate(zip(rx,ry,right_faces,rtx,rty)):
            face_x, face_y = face
            n = len(face_x)+1 # include center
            hex_verts_x.append( x ) # center
            hex_verts_y.append( y ) # center
            hex_verts_x.extend( list(face_x) )
            hex_verts_y.extend( list(face_y) )
            hex_verts_tx.extend( [tx]*n )
            hex_verts_ty.extend( [ty]*n )
            for i in range(2,len(face_x)+1):
                tri = [idx_offset,idx_offset+i-1,idx_offset+i]
                hex_idxs.append(tri)
            hex_idxs.append( [idx_offset,idx_offset+len(face_x), idx_offset+1] )
            idx_offset += n

        hex_verts_x = np.array(hex_verts_x)
        hex_verts_y = np.array(hex_verts_y)
        hex_verts_tx = np.array(hex_verts_tx)
        hex_verts_ty = np.array(hex_verts_ty)

        fd = open("coords.rs", mode="w")
        fd.write("""// This file autogenerated by generate_texcoords.py
#[derive(Debug, Copy, Clone)]
pub struct Vert {{
    position: [f32; 2],
    tex_coords: [f32; 2],
}}

// This depends on a macro from the `glium` crate.
implement_vertex!(Vert, position, tex_coords);

pub static VERTEX_DATA: [Vert; {n_verts}] = [\n""".format(n_verts=len(hex_verts_x)))

        for i in range(len(hex_verts_x)):
            fd.write('    // Vertex %d\n' % i)
            fd.write("""    Vert {{
        position: [{x}, {y}],
        tex_coords: [{u}, {v}],
    }},\n""".format(
                x=hex_verts_x[i],
                y=hex_verts_y[i],
                u=hex_verts_tx[i],
                v=hex_verts_ty[i],
                )
            )
            fd.write('\n')

        fd.write("];\n")

        fd.write("""
pub const INDEX_DATA: [u16; %d] = [""" % (len(hex_idxs)*3,))

        for tri_enum, tri_i in enumerate(hex_idxs):
            fd.write('    // Triangle %d\n' % tri_enum)
            fd.write('    ' + ', '.join(map(str, tri_i)) + ',\n')

        fd.write("];\n")

        fd.close()
