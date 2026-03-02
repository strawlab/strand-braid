use super::*;

/// What parameters are optimized during bundle adjustment.
#[derive(Clone, Debug, PartialEq, Copy, clap::ValueEnum, Default)]
pub enum CameraModelType {
    /// Tunes the 3D world points, the camera extrinsic parameters, and the
    /// camera intrinsic parameters including all 5 distortion terms (3 radial
    /// distortions, 2 tangential distortions) in the OpenCV Brown-Conrady
    /// distortion model. The intrinsic model has a single focal length (not fx
    /// and fy).
    OpenCV5,
    /// Tunes the 3D world points, the camera extrinsic parameters, and the
    /// camera intrinsic parameters including 4 distortion terms (2 radial
    /// distortions, 2 tangential distortions) in the OpenCV Brown-Conrady
    /// distortion model. The intrinsic model has a single focal length (not fx
    /// and fy).
    OpenCV4,
    /// Tunes the 3D world points, the camera extrinsic parameters, and the
    /// camera intrinsic parameters with no distortion terms. The intrinsic
    /// model has a single focal length (not fx and fy).
    Linear,
    /// Tunes the 3D world points and the camera extrinsic parameters.  The
    /// intrinsic model can have a separate focal length for x and y directions.
    #[default]
    ExtrinsicsOnly,
}

pub(crate) struct CameraModelTypeInfo {
    pub(crate) num_distortion_params: usize,
    pub(crate) num_intrinsic_params: usize,
    pub(crate) num_extrinsic_params: usize,
    pub(crate) num_fixed_params: usize,
}

impl CameraModelTypeInfo {
    pub(crate) fn num_cam_params(&self) -> usize {
        self.num_intrinsic_params + self.num_extrinsic_params
    }
}

impl CameraModelType {
    pub(crate) fn info(&self) -> CameraModelTypeInfo {
        match self {
            CameraModelType::OpenCV5 => CameraModelTypeInfo {
                num_distortion_params: 5,
                num_intrinsic_params: 3 + 5,
                num_extrinsic_params: 6,
                num_fixed_params: 0,
            },
            CameraModelType::OpenCV4 => CameraModelTypeInfo {
                num_distortion_params: 4,
                num_intrinsic_params: 3 + 4,
                num_extrinsic_params: 6,
                num_fixed_params: 0,
            },
            CameraModelType::Linear => CameraModelTypeInfo {
                num_distortion_params: 0,
                num_intrinsic_params: 3,
                num_extrinsic_params: 6,
                num_fixed_params: 0,
            },
            CameraModelType::ExtrinsicsOnly => CameraModelTypeInfo {
                num_distortion_params: 0,
                num_intrinsic_params: 0,
                num_extrinsic_params: 6,
                num_fixed_params: 9, // fx, fy, cx, cy + 5 distortion
            },
        }
    }
}

impl CameraModelType {
    pub(crate) fn eval_cam_jacobians<F: na::RealField + Float>(
        &self,
        ba: &BundleAdjuster<F>,
        cam_num: NCamsType,
        pt_num: usize,
        j: &mut na::OMatrix<F, Dyn, Dyn>,
        cam_sub: ((usize, usize), (usize, usize)),
    ) {
        let pt = ba.points.column(pt_num);
        let [p_x, p_y, p_z] = [pt.x, pt.y, pt.z];

        let cam = &ba.cams[usize(cam_num)];
        let i = cam.intrinsics();
        let _cx = i.cx();
        let _cy = i.cy();
        let d = i.distortion.opencv_vec().as_slice();
        let [k1, k2, p1, p2, k3] = [d[0], d[1], d[2], d[3], d[4]];

        let e = cam.extrinsics();
        let rquat = e.pose().rotation;
        let abc = rquat.scaled_axis();
        let cc = e.camcenter();
        let [r_x, r_y, r_z] = [abc.x, abc.y, abc.z];
        let [w_x, w_y, w_z] = [cc.x, cc.y, cc.z];

        let num_cam_params = self.info().num_cam_params();

        // jacobian for camera parameters
        let (cam_start, cam_geom) = cam_sub;
        let mut j = j.view_mut(cam_start, cam_geom);
        debug_assert_eq!(j.nrows(), 2);
        debug_assert_eq!(j.ncols(), num_cam_params);

        let zero: F = na::convert(0.0);
        let one: F = na::convert(1.0);
        let two: F = na::convert(2.0);
        let three: F = na::convert(3.0);

        match self {
            CameraModelType::OpenCV5 => {
                let f = i.fx(); // we checked in the constructor that fx == fy
                #[cfg_attr(any(), rustfmt::skip)]
                {
                    let x0 = two*p1;
                    let x1 = p_z - w_z;
                    let x2 = Float::powi(r_x, 2);
                    let x3 = Float::powi(r_y, 2);
                    let x4 = Float::powi(r_z, 2);
                    let x5 = x2 + x3 + x4;
                    let x6 = Float::sqrt(x5);
                    let x7 = Float::sin(x6);
                    let x8 = x7/x6;
                    let x9 = r_y*x8;
                    let x10 = Float::recip(x5);
                    let x11 = Float::cos(x6);
                    let x12 = one - x11;
                    let x13 = x10*x12;
                    let x14 = r_z*x13;
                    let x15 = r_x*x14;
                    let x16 = x15 + x9;
                    let x17 = p_y - w_y;
                    let x18 = r_z*x8;
                    let x19 = r_y*x13;
                    let x20 = r_x*x19;
                    let x21 = -x18 + x20;
                    let x22 = p_x - w_x;
                    let x23 = x13*x3;
                    let x24 = x13*x4;
                    let x25 = x23 + x24 - one;
                    let x26 = x1*x16 + x17*x21 - x22*x25;
                    let x27 = r_x*x8;
                    let x28 = r_y*x14;
                    let x29 = x27 + x28;
                    let x30 = x15 - x9;
                    let x31 = x13*x2;
                    let x32 = x31 - one;
                    let x33 = -x23 - x32;
                    let x34 = x1*x33 + x17*x29 + x22*x30;
                    let x35 = Float::powi(x34, -2);
                    let x36 = x18 + x20;
                    let x37 = -r_y*r_z*x10*x12 + x27;
                    let x38 = x24 + x32;
                    let x39 = -x1*x37 - x17*x38 + x22*x36;
                    let x40 = x35*x39;
                    let x41 = x26*x40;
                    let x42 = Float::powi(x39, 2);
                    let x43 = x35*x42;
                    let x44 = Float::powi(x26, 2);
                    let x45 = x35*x44;
                    let x46 = x43 + three*x45;
                    let x47 = x43 + x45;
                    let x48 = Float::powi(x47, 2);
                    let x49 = Float::powi(x47, 3);
                    let x50 = k1*x47 + k2*x48 + k3*x49 + one;
                    let x51 = Float::recip(x34);
                    let x52 = x26*x51;
                    let x53 = f*x52;
                    let x54 = two*x41;
                    let x55 = -f*x54;
                    let x56 = Float::powf(x5,-three/two);
                    let x57 = x56*x7;
                    let x58 = x3*x57;
                    let x59 = r_x*x58;
                    let x60 = x4*x57;
                    let x61 = r_x*x60;
                    let x62 = Float::powi(x5, -2);
                    let x63 = two*x12*x62;
                    let x64 = r_x*x63;
                    let x65 = x3*x64;
                    let x66 = x4*x64;
                    let x67 = x22*(-x59 - x61 + x65 + x66);
                    let x68 = x2*x57;
                    let x69 = r_y*x68;
                    let x70 = x2*x63;
                    let x71 = r_y*x70;
                    let x72 = x69 - x71;
                    let x73 = x19 + x72;
                    let x74 = r_x*r_z;
                    let x75 = x57*x74;
                    let x76 = x10*x11;
                    let x77 = x74*x76;
                    let x78 = x75 - x77;
                    let x79 = x17*(x73 + x78);
                    let x80 = r_z*x68;
                    let x81 = r_z*x70;
                    let x82 = x80 - x81;
                    let x83 = x14 + x82;
                    let x84 = r_x*r_y;
                    let x85 = x76*x84;
                    let x86 = x57*x84;
                    let x87 = x85 - x86;
                    let x88 = x1*(x83 + x87);
                    let x89 = x67 + x79 + x88;
                    let x90 = x0*x40;
                    let x91 = x2*x76 - x68 + x8;
                    let x92 = r_z*x63;
                    let x93 = x84*x92;
                    let x94 = -r_x*r_y*r_z*x56*x7 + x93;
                    let x95 = x1*(-x91 - x94);
                    let x96 = -x75 + x77;
                    let x97 = x22*(x73 + x96);
                    let x98 = Float::powi(r_x, 3);
                    let x99 = r_x*x13;
                    let x100 = -two*x12*x62*x98 + x57*x98 + two*x99;
                    let x101 = x61 - x66;
                    let x102 = x17*(-x100 - x101);
                    let x103 = x102 + x95 + x97;
                    let x104 = x26*x35;
                    let x105 = x0*x104;
                    let x106 = Float::powi(x34, -3);
                    let x107 = r_z*x86 - x93;
                    let x108 = x17*(x107 + x91);
                    let x109 = -x85 + x86;
                    let x110 = x22*(x109 + x83);
                    let x111 = x59 - x65;
                    let x112 = x1*(-x100 - x111);
                    let x113 = x106*(-two*x108 - two*x110 - two*x112);
                    let x114 = x26*x39;
                    let x115 = x0*x114;
                    let x116 = x50*x51;
                    let x117 = -x108 - x110 - x112;
                    let x118 = x104*x50;
                    let x119 = x113*x42;
                    let x120 = x40*(two*x102 + two*x95 + two*x97);
                    let x121 = x119 + x120;
                    let x122 = x113*x44;
                    let x123 = x104*(two*x67 + two*x79 + two*x88);
                    let x124 = three*x122 + three*x123;
                    let x125 = x122 + x123;
                    let x126 = k2*x47;
                    let x127 = three*x119 + three*x120;
                    let x128 = k3*x48;
                    let x129 = k1*(x121 + x125) + x126*(two*x119 + two*x120 + two*x122 + two*x123) + x128*(x124 + x127);
                    let x130 = r_y*x60;
                    let x131 = r_y*x4*x63;
                    let x132 = x17*(-x130 + x131 - x69 + x71);
                    let x133 = x111 + x99;
                    let x134 = r_y*r_z;
                    let x135 = x134*x76;
                    let x136 = x134*x57;
                    let x137 = x135 - x136;
                    let x138 = x22*(x133 + x137);
                    let x139 = r_z*x58;
                    let x140 = x3*x92;
                    let x141 = x139 - x140;
                    let x142 = x14 + x141;
                    let x143 = x1*(x109 + x142);
                    let x144 = x132 + x138 + x143;
                    let x145 = x3*x76 - x58;
                    let x146 = x107 + x8;
                    let x147 = x1*(x145 + x146);
                    let x148 = -x135 + x136;
                    let x149 = x17*(x133 + x148);
                    let x150 = Float::powi(r_y, 3);
                    let x151 = -two*x12*x150*x62 + x150*x57 + two*x19;
                    let x152 = x130 - x131;
                    let x153 = x22*(-x151 - x152);
                    let x154 = x147 + x149 + x153;
                    let x155 = x8 + x94;
                    let x156 = x22*(-x145 - x155);
                    let x157 = x17*(x142 + x87);
                    let x158 = x1*(-x151 - x72);
                    let x159 = x106*(-two*x156 - two*x157 - two*x158);
                    let x160 = -x156 - x157 - x158;
                    let x161 = x159*x42;
                    let x162 = x40*(two*x132 + two*x138 + two*x143);
                    let x163 = x161 + x162;
                    let x164 = x159*x44;
                    let x165 = x104*(two*x147 + two*x149 + two*x153);
                    let x166 = three*x164 + three*x165;
                    let x167 = x164 + x165;
                    let x168 = three*x161 + three*x162;
                    let x169 = k1*(x163 + x167) + x126*(two*x161 + two*x162 + two*x164 + two*x165) + x128*(x166 + x168);
                    let x170 = x4*x76 - x60;
                    let x171 = x22*(x146 + x170);
                    let x172 = x152 + x19;
                    let x173 = x1*(x172 + x78);
                    let x174 = Float::powi(r_z, 3);
                    let x175 = -two*x12*x174*x62 + two*x14 + x174*x57;
                    let x176 = x17*(-x175 - x82);
                    let x177 = x171 + x173 + x176;
                    let x178 = x17*(-x155 - x170);
                    let x179 = x101 + x99;
                    let x180 = x1*(x137 + x179);
                    let x181 = x22*(-x141 - x175);
                    let x182 = x178 + x180 + x181;
                    let x183 = x1*(-x139 + x140 - x80 + x81);
                    let x184 = x22*(x148 + x179);
                    let x185 = x17*(x172 + x96);
                    let x186 = x106*(-two*x183 - two*x184 - two*x185);
                    let x187 = -x183 - x184 - x185;
                    let x188 = x186*x42;
                    let x189 = x40*(two*x171 + two*x173 + two*x176);
                    let x190 = x188 + x189;
                    let x191 = x186*x44;
                    let x192 = x104*(two*x178 + two*x180 + two*x181);
                    let x193 = three*x191 + three*x192;
                    let x194 = x191 + x192;
                    let x195 = three*x188 + three*x189;
                    let x196 = k1*(x190 + x194) + x126*(two*x188 + two*x189 + two*x191 + two*x192) + x128*(x193 + x195);
                    let x197 = -x36;
                    let x198 = two*x9;
                    let x199 = two*x15;
                    let x200 = x106*(-x198 + x199);
                    let x201 = x200*x42;
                    let x202 = two*x18;
                    let x203 = two*x20;
                    let x204 = x40*(-x202 - x203);
                    let x205 = x201 + x204;
                    let x206 = x200*x44;
                    let x207 = two*x23;
                    let x208 = two*x24 - two;
                    let x209 = x104*(x207 + x208);
                    let x210 = three*x206 + three*x209;
                    let x211 = x206 + x209;
                    let x212 = three*x201 + three*x204;
                    let x213 = k1*(x205 + x211) + x126*(two*x201 + two*x204 + two*x206 + two*x209) + x128*(x210 + x212);
                    let x214 = -x21;
                    let x215 = two*x27;
                    let x216 = two*x28;
                    let x217 = x106*(x215 + x216);
                    let x218 = x217*x42;
                    let x219 = two*x31;
                    let x220 = x40*(x208 + x219);
                    let x221 = x218 + x220;
                    let x222 = x217*x44;
                    let x223 = x104*(x202 - x203);
                    let x224 = three*x222 + three*x223;
                    let x225 = x222 + x223;
                    let x226 = three*x218 + three*x220;
                    let x227 = k1*(x221 + x225) + x126*(two*x218 + two*x220 + two*x222 + two*x223) + x128*(x224 + x226);
                    let x228 = -x16;
                    let x229 = x106*(-x207 - x219 + two);
                    let x230 = x229*x42;
                    let x231 = x40*(x215 - x216);
                    let x232 = x230 + x231;
                    let x233 = x229*x44;
                    let x234 = x104*(-x198 - x199);
                    let x235 = three*x233 + three*x234;
                    let x236 = x233 + x234;
                    let x237 = three*x230 + three*x231;
                    let x238 = k1*(x232 + x236) + x126*(two*x230 + two*x231 + two*x233 + two*x234) + x128*(x235 + x237);
                    let x239 = three*x43 + x45;
                    let x240 = x39*x51;
                    let x241 = f*x240;
                    let x242 = two*p2;
                    let x243 = x242*x40;
                    let x244 = x104*x242;
                    let x245 = x114*x242;
                    let x246 = x40*x50;

                    // first row of Jacobian, derivatives of first residual (u)
                    j[(0,0)] = -p2*x46 - x0*x41 - x50*x52;
                    j[(0,1)] = -one;
                    j[(0,2)] = zero;
                    j[(0,3)] = -x47*x53;
                    j[(0,4)] = -x48*x53;
                    j[(0,5)] = x55;
                    j[(0,6)] = -f*x46;
                    j[(0,7)] = -x49*x53;
                    j[(0,8)] = -f*(p2*(x121 + x124) + x103*x105 + x113*x115 + x116*x89 + x117*x118 + x129*x52 + x89*x90);
                    j[(0,9)] = -f*(p2*(x163 + x166) + x105*x144 + x115*x159 + x116*x154 + x118*x160 + x154*x90 + x169*x52);
                    j[(0,10)] = -f*(p2*(x190 + x193) + x105*x177 + x115*x186 + x116*x182 + x118*x187 + x182*x90 + x196*x52);
                    j[(0,11)] = -f*(p2*(x205 + x210) + x105*x197 + x115*x200 + x116*x25 + x118*x30 + x213*x52 + x25*x90);
                    j[(0,12)] = -f*(p2*(x221 + x224) + x105*x38 + x115*x217 + x116*x214 + x118*x29 + x214*x90 + x227*x52);
                    j[(0,13)] = -f*(p2*(x232 + x235) + x105*x37 + x115*x229 + x116*x228 + x118*x33 + x228*x90 + x238*x52);

                    // second row of Jacobian, derivatives of second residual (v)
                    j[(1,0)] = -p1*x239 - p2*x54 - x116*x39;
                    j[(1,1)] = zero;
                    j[(1,2)] = -one;
                    j[(1,3)] = -x241*x47;
                    j[(1,4)] = -x241*x48;
                    j[(1,5)] = -f*x239;
                    j[(1,6)] = x55;
                    j[(1,7)] = -x241*x49;
                    j[(1,8)] = -f*(p1*(x125 + x127) + x103*x116 + x103*x244 + x113*x245 + x117*x246 + x129*x240 + x243*x89);
                    j[(1,9)] = -f*(p1*(x167 + x168) + x116*x144 + x144*x244 + x154*x243 + x159*x245 + x160*x246 + x169*x240);
                    j[(1,10)] = -f*(p1*(x194 + x195) + x116*x177 + x177*x244 + x182*x243 + x186*x245 + x187*x246 + x196*x240);
                    j[(1,11)] = -f*(p1*(x211 + x212) + x116*x197 + x197*x244 + x200*x245 + x213*x240 + x243*x25 + x246*x30);
                    j[(1,12)] = -f*(p1*(x225 + x226) + x116*x38 + x214*x243 + x217*x245 + x227*x240 + x244*x38 + x246*x29);
                    j[(1,13)] = -f*(p1*(x236 + x237) + x116*x37 + x228*x243 + x229*x245 + x238*x240 + x244*x37 + x246*x33);
                }
            }

            CameraModelType::OpenCV4 => {
                let f = i.fx(); // we checked in the constructor that fx == fy
                #[cfg_attr(any(), rustfmt::skip)]
                {
                    let x0 = two*p1;
                    let x1 = p_z - w_z;
                    let x2 = Float::powi(r_x, 2);
                    let x3 = Float::powi(r_y, 2);
                    let x4 = Float::powi(r_z, 2);
                    let x5 = x2 + x3 + x4;
                    let x6 = Float::sqrt(x5);
                    let x7 = Float::sin(x6);
                    let x8 = x7/x6;
                    let x9 = r_y*x8;
                    let x10 = Float::recip(x5);
                    let x11 = Float::cos(x6);
                    let x12 = one - x11;
                    let x13 = x10*x12;
                    let x14 = r_z*x13;
                    let x15 = r_x*x14;
                    let x16 = x15 + x9;
                    let x17 = p_y - w_y;
                    let x18 = r_z*x8;
                    let x19 = r_y*x13;
                    let x20 = r_x*x19;
                    let x21 = -x18 + x20;
                    let x22 = p_x - w_x;
                    let x23 = x13*x3;
                    let x24 = x13*x4;
                    let x25 = x23 + x24 - one;
                    let x26 = x1*x16 + x17*x21 - x22*x25;
                    let x27 = r_x*x8;
                    let x28 = r_y*x14;
                    let x29 = x27 + x28;
                    let x30 = x15 - x9;
                    let x31 = x13*x2;
                    let x32 = x31 - one;
                    let x33 = -x23 - x32;
                    let x34 = x1*x33 + x17*x29 + x22*x30;
                    let x35 = Float::powi(x34, -2);
                    let x36 = x18 + x20;
                    let x37 = -r_y*r_z*x10*x12 + x27;
                    let x38 = x24 + x32;
                    let x39 = -x1*x37 - x17*x38 + x22*x36;
                    let x40 = x35*x39;
                    let x41 = x26*x40;
                    let x42 = Float::powi(x39, 2);
                    let x43 = x35*x42;
                    let x44 = Float::powi(x26, 2);
                    let x45 = x35*x44;
                    let x46 = x43 + three*x45;
                    let x47 = x43 + x45;
                    let x48 = Float::powi(x47, 2);
                    let x49 = k1*x47 + k2*x48 + one;
                    let x50 = Float::recip(x34);
                    let x51 = x26*x50;
                    let x52 = f*x51;
                    let x53 = two*x41;
                    let x54 = -f*x53;
                    let x55 = Float::powf(x5, -three/two);
                    let x56 = x55*x7;
                    let x57 = x3*x56;
                    let x58 = r_x*x57;
                    let x59 = x4*x56;
                    let x60 = r_x*x59;
                    let x61 = Float::powi(x5, -2);
                    let x62 = two*x12*x61;
                    let x63 = r_x*x62;
                    let x64 = x3*x63;
                    let x65 = x4*x63;
                    let x66 = x22*(-x58 - x60 + x64 + x65);
                    let x67 = x2*x56;
                    let x68 = r_y*x67;
                    let x69 = x2*x62;
                    let x70 = r_y*x69;
                    let x71 = x68 - x70;
                    let x72 = x19 + x71;
                    let x73 = r_x*r_z;
                    let x74 = x56*x73;
                    let x75 = x10*x11;
                    let x76 = x73*x75;
                    let x77 = x74 - x76;
                    let x78 = x17*(x72 + x77);
                    let x79 = r_z*x67;
                    let x80 = r_z*x69;
                    let x81 = x79 - x80;
                    let x82 = x14 + x81;
                    let x83 = r_x*r_y;
                    let x84 = x75*x83;
                    let x85 = x56*x83;
                    let x86 = x84 - x85;
                    let x87 = x1*(x82 + x86);
                    let x88 = x66 + x78 + x87;
                    let x89 = x0*x40;
                    let x90 = x2*x75 - x67 + x8;
                    let x91 = r_z*x62;
                    let x92 = x83*x91;
                    let x93 = -r_x*r_y*r_z*x55*x7 + x92;
                    let x94 = x1*(-x90 - x93);
                    let x95 = -x74 + x76;
                    let x96 = x22*(x72 + x95);
                    let x97 = Float::powi(r_x, 3);
                    let x98 = r_x*x13;
                    let x99 = -two*x12*x61*x97 + x56*x97 + two*x98;
                    let x100 = x60 - x65;
                    let x101 = x17*(-x100 - x99);
                    let x102 = x101 + x94 + x96;
                    let x103 = x26*x35;
                    let x104 = x0*x103;
                    let x105 = Float::powi(x34, -3);
                    let x106 = r_z*x85 - x92;
                    let x107 = x17*(x106 + x90);
                    let x108 = -x84 + x85;
                    let x109 = x22*(x108 + x82);
                    let x110 = x58 - x64;
                    let x111 = x1*(-x110 - x99);
                    let x112 = x105*(-two*x107 - two*x109 - two*x111);
                    let x113 = x26*x39;
                    let x114 = x0*x113;
                    let x115 = x49*x50;
                    let x116 = -x107 - x109 - x111;
                    let x117 = x103*x49;
                    let x118 = x112*x44;
                    let x119 = x103*(two*x66 + two*x78 + two*x87);
                    let x120 = x112*x42;
                    let x121 = x40*(two*x101 + two*x94 + two*x96);
                    let x122 = x120 + x121;
                    let x123 = x118 + x119;
                    let x124 = k2*x47;
                    let x125 = k1*(x122 + x123) + x124*(two*x118 + two*x119 + two*x120 + two*x121);
                    let x126 = r_y*x59;
                    let x127 = r_y*x4*x62;
                    let x128 = x17*(-x126 + x127 - x68 + x70);
                    let x129 = x110 + x98;
                    let x130 = r_y*r_z;
                    let x131 = x130*x75;
                    let x132 = x130*x56;
                    let x133 = x131 - x132;
                    let x134 = x22*(x129 + x133);
                    let x135 = r_z*x57;
                    let x136 = x3*x91;
                    let x137 = x135 - x136;
                    let x138 = x137 + x14;
                    let x139 = x1*(x108 + x138);
                    let x140 = x128 + x134 + x139;
                    let x141 = x3*x75 - x57;
                    let x142 = x106 + x8;
                    let x143 = x1*(x141 + x142);
                    let x144 = -x131 + x132;
                    let x145 = x17*(x129 + x144);
                    let x146 = Float::powi(r_y, 3);
                    let x147 = -two*x12*x146*x61 + x146*x56 + two*x19;
                    let x148 = x126 - x127;
                    let x149 = x22*(-x147 - x148);
                    let x150 = x143 + x145 + x149;
                    let x151 = x8 + x93;
                    let x152 = x22*(-x141 - x151);
                    let x153 = x17*(x138 + x86);
                    let x154 = x1*(-x147 - x71);
                    let x155 = x105*(-two*x152 - two*x153 - two*x154);
                    let x156 = -x152 - x153 - x154;
                    let x157 = x155*x44;
                    let x158 = x103*(two*x143 + two*x145 + two*x149);
                    let x159 = x155*x42;
                    let x160 = x40*(two*x128 + two*x134 + two*x139);
                    let x161 = x159 + x160;
                    let x162 = x157 + x158;
                    let x163 = k1*(x161 + x162) + x124*(two*x157 + two*x158 + two*x159 + two*x160);
                    let x164 = x4*x75 - x59;
                    let x165 = x22*(x142 + x164);
                    let x166 = x148 + x19;
                    let x167 = x1*(x166 + x77);
                    let x168 = Float::powi(r_z, 3);
                    let x169 = -two*x12*x168*x61 + two*x14 + x168*x56;
                    let x170 = x17*(-x169 - x81);
                    let x171 = x165 + x167 + x170;
                    let x172 = x17*(-x151 - x164);
                    let x173 = x100 + x98;
                    let x174 = x1*(x133 + x173);
                    let x175 = x22*(-x137 - x169);
                    let x176 = x172 + x174 + x175;
                    let x177 = x1*(-x135 + x136 - x79 + x80);
                    let x178 = x22*(x144 + x173);
                    let x179 = x17*(x166 + x95);
                    let x180 = x105*(-two*x177 - two*x178 - two*x179);
                    let x181 = -x177 - x178 - x179;
                    let x182 = x180*x44;
                    let x183 = x103*(two*x172 + two*x174 + two*x175);
                    let x184 = x180*x42;
                    let x185 = x40*(two*x165 + two*x167 + two*x170);
                    let x186 = x184 + x185;
                    let x187 = x182 + x183;
                    let x188 = k1*(x186 + x187) + x124*(two*x182 + two*x183 + two*x184 + two*x185);
                    let x189 = -x36;
                    let x190 = two*x9;
                    let x191 = two*x15;
                    let x192 = x105*(-x190 + x191);
                    let x193 = x192*x44;
                    let x194 = two*x23;
                    let x195 = two*x24 - two;
                    let x196 = x103*(x194 + x195);
                    let x197 = x192*x42;
                    let x198 = two*x18;
                    let x199 = two*x20;
                    let x200 = x40*(-x198 - x199);
                    let x201 = x197 + x200;
                    let x202 = x193 + x196;
                    let x203 = k1*(x201 + x202) + x124*(two*x193 + two*x196 + two*x197 + two*x200);
                    let x204 = -x21;
                    let x205 = two*x27;
                    let x206 = two*x28;
                    let x207 = x105*(x205 + x206);
                    let x208 = x207*x44;
                    let x209 = x103*(x198 - x199);
                    let x210 = x207*x42;
                    let x211 = two*x31;
                    let x212 = x40*(x195 + x211);
                    let x213 = x210 + x212;
                    let x214 = x208 + x209;
                    let x215 = k1*(x213 + x214) + x124*(two*x208 + two*x209 + two*x210 + two*x212);
                    let x216 = -x16;
                    let x217 = x105*(-x194 - x211 + two);
                    let x218 = x217*x44;
                    let x219 = x103*(-x190 - x191);
                    let x220 = x217*x42;
                    let x221 = x40*(x205 - x206);
                    let x222 = x220 + x221;
                    let x223 = x218 + x219;
                    let x224 = k1*(x222 + x223) + x124*(two*x218 + two*x219 + two*x220 + two*x221);
                    let x225 = three*x43 + x45;
                    let x226 = x39*x50;
                    let x227 = f*x226;
                    let x228 = two*p2;
                    let x229 = x228*x40;
                    let x230 = x103*x228;
                    let x231 = x113*x228;
                    let x232 = x40*x49;
                    //final jacobian: (shape: 2 x 13)
                    j[(0,0)] = -p2*x46 - x0*x41 - x49*x51;
                    j[(0,1)] = -one;
                    j[(0,2)] = zero;
                    j[(0,3)] = -x47*x52;
                    j[(0,4)] = -x48*x52;
                    j[(0,5)] = x54;
                    j[(0,6)] = -f*x46;
                    j[(0,7)] = -f*(p2*(three*x118 + three*x119 + x122) + x102*x104 + x112*x114 + x115*x88 + x116*x117 + x125*x51 + x88*x89);
                    j[(0,8)] = -f*(p2*(three*x157 + three*x158 + x161) + x104*x140 + x114*x155 + x115*x150 + x117*x156 + x150*x89 + x163*x51);
                    j[(0,9)] = -f*(p2*(three*x182 + three*x183 + x186) + x104*x171 + x114*x180 + x115*x176 + x117*x181 + x176*x89 + x188*x51);
                    j[(0,10)] = -f*(p2*(three*x193 + three*x196 + x201) + x104*x189 + x114*x192 + x115*x25 + x117*x30 + x203*x51 + x25*x89);
                    j[(0,11)] = -f*(p2*(three*x208 + three*x209 + x213) + x104*x38 + x114*x207 + x115*x204 + x117*x29 + x204*x89 + x215*x51);
                    j[(0,12)] = -f*(p2*(three*x218 + three*x219 + x222) + x104*x37 + x114*x217 + x115*x216 + x117*x33 + x216*x89 + x224*x51);
                    j[(1,0)] = -p1*x225 - p2*x53 - x115*x39;
                    j[(1,1)] = zero;
                    j[(1,2)] = -one;
                    j[(1,3)] = -x227*x47;
                    j[(1,4)] = -x227*x48;
                    j[(1,5)] = -f*x225;
                    j[(1,6)] = x54;
                    j[(1,7)] = -f*(p1*(three*x120 + three*x121 + x123) + x102*x115 + x102*x230 + x112*x231 + x116*x232 + x125*x226 + x229*x88);
                    j[(1,8)] = -f*(p1*(three*x159 + three*x160 + x162) + x115*x140 + x140*x230 + x150*x229 + x155*x231 + x156*x232 + x163*x226);
                    j[(1,9)] = -f*(p1*(three*x184 + three*x185 + x187) + x115*x171 + x171*x230 + x176*x229 + x180*x231 + x181*x232 + x188*x226);
                    j[(1,10)] = -f*(p1*(three*x197 + three*x200 + x202) + x115*x189 + x189*x230 + x192*x231 + x203*x226 + x229*x25 + x232*x30);
                    j[(1,11)] = -f*(p1*(three*x210 + three*x212 + x214) + x115*x38 + x204*x229 + x207*x231 + x215*x226 + x230*x38 + x232*x29);
                    j[(1,12)] = -f*(p1*(three*x220 + three*x221 + x223) + x115*x37 + x216*x229 + x217*x231 + x224*x226 + x230*x37 + x232*x33);
                }
            }

            CameraModelType::Linear => {
                let f = i.fx(); // we checked in the constructor that fx == fy
                #[cfg_attr(any(), rustfmt::skip)]
                {
                    let x0 = p_z - w_z;
                    let x1 = Float::powi(r_x, 2);
                    let x2 = Float::powi(r_y, 2);
                    let x3 = Float::powi(r_z, 2);
                    let x4 = x1 + x2 + x3;
                    let x5 = Float::sqrt(x4);
                    let x6 = Float::sin(x5);
                    let x7 = x6/x5;
                    let x8 = r_y*x7;
                    let x9 = Float::recip(x4);
                    let x10 = Float::cos(x5);
                    let x11 = one - x10;
                    let x12 = x11*x9;
                    let x13 = r_z*x12;
                    let x14 = r_x*x13;
                    let x15 = x14 + x8;
                    let x16 = p_y - w_y;
                    let x17 = r_z*x7;
                    let x18 = r_y*x12;
                    let x19 = r_x*x18;
                    let x20 = -x17 + x19;
                    let x21 = p_x - w_x;
                    let x22 = x12*x3;
                    let x23 = x12*x2 - one;
                    let x24 = x22 + x23;
                    let x25 = x0*x15 + x16*x20 - x21*x24;
                    let x26 = r_x*x7;
                    let x27 = r_z*x18 + x26;
                    let x28 = x14 - x8;
                    let x29 = x1*x12;
                    let x30 = -x23 - x29;
                    let x31 = x0*x30 + x16*x27 + x21*x28;
                    let x32 = Float::recip(x31);
                    let x33 = Float::powf(x4, -three/two);
                    let x34 = x33*x6;
                    let x35 = x2*x34;
                    let x36 = r_x*x35;
                    let x37 = x3*x34;
                    let x38 = r_x*x37;
                    let x39 = Float::powi(x4, -2);
                    let x40 = two*x11*x39;
                    let x41 = r_x*x40;
                    let x42 = x2*x41;
                    let x43 = x3*x41;
                    let x44 = x1*x34;
                    let x45 = r_y*x44;
                    let x46 = x1*x40;
                    let x47 = r_y*x46;
                    let x48 = x45 - x47;
                    let x49 = x18 + x48;
                    let x50 = r_x*r_z;
                    let x51 = x34*x50;
                    let x52 = x10*x9;
                    let x53 = x50*x52;
                    let x54 = x51 - x53;
                    let x55 = r_z*x44;
                    let x56 = r_z*x46;
                    let x57 = x55 - x56;
                    let x58 = x13 + x57;
                    let x59 = r_x*r_y;
                    let x60 = x52*x59;
                    let x61 = x34*x59;
                    let x62 = x60 - x61;
                    let x63 = f*x32;
                    let x64 = r_y*x40;
                    let x65 = x50*x64;
                    let x66 = r_y*x51 - x65 + x7;
                    let x67 = x1*x52 - x44;
                    let x68 = -x60 + x61;
                    let x69 = x36 - x42;
                    let x70 = Float::powi(r_x, 3);
                    let x71 = r_x*x12;
                    let x72 = -two*x11*x39*x70 + x34*x70 + two*x71;
                    let x73 = -x0*(-x69 - x72) - x16*(x66 + x67) - x21*(x58 + x68);
                    let x74 = f/Float::powi(x31, 2);
                    let x75 = x25*x74;
                    let x76 = x2*x52 - x35;
                    let x77 = x69 + x71;
                    let x78 = r_y*r_z;
                    let x79 = x34*x78;
                    let x80 = x52*x78;
                    let x81 = x79 - x80;
                    let x82 = r_y*x37;
                    let x83 = x3*x64;
                    let x84 = x82 - x83;
                    let x85 = Float::powi(r_y, 3);
                    let x86 = -two*x11*x39*x85 + two*x18 + x34*x85;
                    let x87 = -r_x*r_y*r_z*x33*x6 + x65 + x7;
                    let x88 = r_z*x35;
                    let x89 = r_z*x2*x40;
                    let x90 = x88 - x89;
                    let x91 = x13 + x90;
                    let x92 = -x0*(-x48 - x86) - x16*(x62 + x91) - x21*(-x76 - x87);
                    let x93 = x3*x52 - x37;
                    let x94 = -x79 + x80;
                    let x95 = x38 - x43;
                    let x96 = x71 + x95;
                    let x97 = Float::powi(r_z, 3);
                    let x98 = -two*x11*x39*x97 + two*x13 + x34*x97;
                    let x99 = -x51 + x53;
                    let x100 = x18 + x84;
                    let x101 = -x0*(-x55 + x56 - x88 + x89) - x16*(x100 + x99) - x21*(x81 + x96);
                    let x102 = x17 + x19;
                    let x103 = -r_y*r_z*x11*x9 + x26;
                    let x104 = x22 + x29 - one;
                    let x105 = -x0*x103 + x102*x21 - x104*x16;
                    let x106 = x105*x74;
                    //final jacobian: (shape: 2 x 9)
                    j[(0,0)] = -x25*x32;
                    j[(0,1)] = -one;
                    j[(0,2)] = zero;
                    j[(0,3)] = -x63*(x0*(x58 + x62) + x16*(x49 + x54) + x21*(-x36 - x38 + x42 + x43)) - x73*x75;
                    j[(0,4)] = -x63*(x0*(x66 + x76) + x16*(x77 + x81) + x21*(-x84 - x86)) - x75*x92;
                    j[(0,5)] = -x101*x75 - x63*(x0*(x94 + x96) + x16*(-x87 - x93) + x21*(-x90 - x98));
                    j[(0,6)] = -x24*x63 - x28*x75;
                    j[(0,7)] = x20*x63 - x27*x75;
                    j[(0,8)] = x15*x63 - x30*x75;
                    j[(1,0)] = -x105*x32;
                    j[(1,1)] = zero;
                    j[(1,2)] = -one;
                    j[(1,3)] = -x106*x73 - x63*(x0*(-x67 - x87) + x16*(-x72 - x95) + x21*(x49 + x99));
                    j[(1,4)] = -x106*x92 - x63*(x0*(x68 + x91) + x16*(-x45 + x47 - x82 + x83) + x21*(x77 + x94));
                    j[(1,5)] = -x101*x106 - x63*(x0*(x100 + x54) + x16*(-x57 - x98) + x21*(x66 + x93));
                    j[(1,6)] = x102*x63 - x106*x28;
                    j[(1,7)] = -x104*x63 - x106*x27;
                    j[(1,8)] = -x103*x63 - x106*x30;
                }
            }
            CameraModelType::ExtrinsicsOnly => {
                let fx = i.fx();
                let fy = i.fy();
                #[cfg_attr(any(), rustfmt::skip)]
                {
                    let x0 = p_x - w_x;
                    let x1 = Float::powi(r_y, 2);
                    let x2 = Float::powi(r_x, 2);
                    let x3 = Float::powi(r_z, 2);
                    let x4 = x1 + x2 + x3;
                    let x5 = Float::sqrt(x4);
                    let x6 = Float::sin(x5);
                    let x7 = Float::powf(x4, -three/two);
                    let x8 = x6*x7;
                    let x9 = x1*x8;
                    let x10 = r_x*x9;
                    let x11 = x3*x8;
                    let x12 = r_x*x11;
                    let x13 = Float::cos(x5);
                    let x14 = one - x13;
                    let x15 = Float::powi(x4, -2);
                    let x16 = two*x14*x15;
                    let x17 = r_x*x16;
                    let x18 = x1*x17;
                    let x19 = x17*x3;
                    let x20 = x0*(-x10 - x12 + x18 + x19);
                    let x21 = p_y - w_y;
                    let x22 = Float::recip(x4);
                    let x23 = x14*x22;
                    let x24 = r_y*x23;
                    let x25 = x2*x8;
                    let x26 = r_y*x25;
                    let x27 = x16*x2;
                    let x28 = r_y*x27;
                    let x29 = x26 - x28;
                    let x30 = x24 + x29;
                    let x31 = r_x*r_z;
                    let x32 = x31*x8;
                    let x33 = x13*x22;
                    let x34 = x31*x33;
                    let x35 = x32 - x34;
                    let x36 = x21*(x30 + x35);
                    let x37 = p_z - w_z;
                    let x38 = r_z*x23;
                    let x39 = r_z*x25;
                    let x40 = r_z*x27;
                    let x41 = x39 - x40;
                    let x42 = x38 + x41;
                    let x43 = r_x*r_y;
                    let x44 = x33*x43;
                    let x45 = x43*x8;
                    let x46 = x44 - x45;
                    let x47 = x37*(x42 + x46);
                    let x48 = x20 + x36 + x47;
                    let x49 = x6/x5;
                    let x50 = r_x*x49;
                    let x51 = r_y*x38;
                    let x52 = x50 + x51;
                    let x53 = r_y*x49;
                    let x54 = r_x*x38;
                    let x55 = -x53 + x54;
                    let x56 = x1*x23;
                    let x57 = x2*x23;
                    let x58 = x57 - one;
                    let x59 = -x56 - x58;
                    let x60 = x0*x55 + x21*x52 + x37*x59;
                    let x61 = Float::powi(x60, -2);
                    let x62 = r_z*x49;
                    let x63 = r_x*x24;
                    let x64 = x62 + x63;
                    let x65 = -r_y*r_z*x14*x22 + x50;
                    let x66 = x23*x3;
                    let x67 = x58 + x66;
                    let x68 = x0*x64 - x21*x67 - x37*x65;
                    let x69 = x61*x68;
                    let x70 = two*p1;
                    let x71 = x69*x70;
                    let x72 = x2*x33 - x25 + x49;
                    let x73 = r_z*x16;
                    let x74 = x43*x73;
                    let x75 = -r_x*r_y*r_z*x6*x7 + x74;
                    let x76 = x37*(-x72 - x75);
                    let x77 = -x32 + x34;
                    let x78 = x0*(x30 + x77);
                    let x79 = Float::powi(r_x, 3);
                    let x80 = r_x*x23;
                    let x81 = -two*x14*x15*x79 + x79*x8 + two*x80;
                    let x82 = x12 - x19;
                    let x83 = x21*(-x81 - x82);
                    let x84 = x76 + x78 + x83;
                    let x85 = x53 + x54;
                    let x86 = -x62 + x63;
                    let x87 = x56 + x66 - one;
                    let x88 = -x0*x87 + x21*x86 + x37*x85;
                    let x89 = x61*x88;
                    let x90 = x70*x89;
                    let x91 = Float::powi(x60, -3);
                    let x92 = r_z*x45 - x74;
                    let x93 = x21*(x72 + x92);
                    let x94 = -x44 + x45;
                    let x95 = x0*(x42 + x94);
                    let x96 = x10 - x18;
                    let x97 = x37*(-x81 - x96);
                    let x98 = x91*(-two*x93 - two*x95 - two*x97);
                    let x99 = x68*x88;
                    let x100 = x70*x99;
                    let x101 = Float::recip(x60);
                    let x102 = Float::powi(x68, 2);
                    let x103 = Float::powi(x88, 2);
                    let x104 = x102*x61 + x103*x61;
                    let x105 = k1*x104 + k2*Float::powi(x104, 2) + one;
                    let x106 = x101*x105;
                    let x107 = -x93 - x95 - x97;
                    let x108 = x105*x89;
                    let x109 = x103*x98;
                    let x110 = x89*(two*x20 + two*x36 + two*x47);
                    let x111 = x102*x98;
                    let x112 = x69*(two*x76 + two*x78 + two*x83);
                    let x113 = x111 + x112;
                    let x114 = x109 + x110;
                    let x115 = k2*x104;
                    let x116 = k1*(x113 + x114) + x115*(two*x109 + two*x110 + two*x111 + two*x112);
                    let x117 = x101*x88;
                    let x118 = r_y*x11;
                    let x119 = r_y*x16*x3;
                    let x120 = x21*(-x118 + x119 - x26 + x28);
                    let x121 = x80 + x96;
                    let x122 = r_y*r_z;
                    let x123 = x122*x33;
                    let x124 = x122*x8;
                    let x125 = x123 - x124;
                    let x126 = x0*(x121 + x125);
                    let x127 = r_z*x9;
                    let x128 = x1*x73;
                    let x129 = x127 - x128;
                    let x130 = x129 + x38;
                    let x131 = x37*(x130 + x94);
                    let x132 = x120 + x126 + x131;
                    let x133 = x1*x33 - x9;
                    let x134 = x49 + x92;
                    let x135 = x37*(x133 + x134);
                    let x136 = -x123 + x124;
                    let x137 = x21*(x121 + x136);
                    let x138 = Float::powi(r_y, 3);
                    let x139 = -two*x138*x14*x15 + x138*x8 + two*x24;
                    let x140 = x118 - x119;
                    let x141 = x0*(-x139 - x140);
                    let x142 = x135 + x137 + x141;
                    let x143 = x49 + x75;
                    let x144 = x0*(-x133 - x143);
                    let x145 = x21*(x130 + x46);
                    let x146 = x37*(-x139 - x29);
                    let x147 = x91*(-two*x144 - two*x145 - two*x146);
                    let x148 = -x144 - x145 - x146;
                    let x149 = x103*x147;
                    let x150 = x89*(two*x135 + two*x137 + two*x141);
                    let x151 = x102*x147;
                    let x152 = x69*(two*x120 + two*x126 + two*x131);
                    let x153 = x151 + x152;
                    let x154 = x149 + x150;
                    let x155 = k1*(x153 + x154) + x115*(two*x149 + two*x150 + two*x151 + two*x152);
                    let x156 = -x11 + x3*x33;
                    let x157 = x0*(x134 + x156);
                    let x158 = x140 + x24;
                    let x159 = x37*(x158 + x35);
                    let x160 = Float::powi(r_z, 3);
                    let x161 = -two*x14*x15*x160 + x160*x8 + two*x38;
                    let x162 = x21*(-x161 - x41);
                    let x163 = x157 + x159 + x162;
                    let x164 = x21*(-x143 - x156);
                    let x165 = x80 + x82;
                    let x166 = x37*(x125 + x165);
                    let x167 = x0*(-x129 - x161);
                    let x168 = x164 + x166 + x167;
                    let x169 = x37*(-x127 + x128 - x39 + x40);
                    let x170 = x0*(x136 + x165);
                    let x171 = x21*(x158 + x77);
                    let x172 = x91*(-two*x169 - two*x170 - two*x171);
                    let x173 = -x169 - x170 - x171;
                    let x174 = x103*x172;
                    let x175 = x89*(two*x164 + two*x166 + two*x167);
                    let x176 = x102*x172;
                    let x177 = x69*(two*x157 + two*x159 + two*x162);
                    let x178 = x176 + x177;
                    let x179 = x174 + x175;
                    let x180 = k1*(x178 + x179) + x115*(two*x174 + two*x175 + two*x176 + two*x177);
                    let x181 = -x64;
                    let x182 = two*x53;
                    let x183 = two*x54;
                    let x184 = x91*(-x182 + x183);
                    let x185 = x103*x184;
                    let x186 = two*x56;
                    let x187 = two*x66 - two;
                    let x188 = x89*(x186 + x187);
                    let x189 = x102*x184;
                    let x190 = two*x62;
                    let x191 = two*x63;
                    let x192 = x69*(-x190 - x191);
                    let x193 = x189 + x192;
                    let x194 = x185 + x188;
                    let x195 = k1*(x193 + x194) + x115*(two*x185 + two*x188 + two*x189 + two*x192);
                    let x196 = -x86;
                    let x197 = two*x50;
                    let x198 = two*x51;
                    let x199 = x91*(x197 + x198);
                    let x200 = x103*x199;
                    let x201 = x89*(x190 - x191);
                    let x202 = x102*x199;
                    let x203 = two*x57;
                    let x204 = x69*(x187 + x203);
                    let x205 = x202 + x204;
                    let x206 = x200 + x201;
                    let x207 = k1*(x205 + x206) + x115*(two*x200 + two*x201 + two*x202 + two*x204);
                    let x208 = -x85;
                    let x209 = x91*(-x186 - x203 + two);
                    let x210 = x103*x209;
                    let x211 = x89*(-x182 - x183);
                    let x212 = x102*x209;
                    let x213 = x69*(x197 - x198);
                    let x214 = x212 + x213;
                    let x215 = x210 + x211;
                    let x216 = k1*(x214 + x215) + x115*(two*x210 + two*x211 + two*x212 + two*x213);
                    let x217 = two*p2;
                    let x218 = x217*x69;
                    let x219 = x217*x89;
                    let x220 = x217*x99;
                    let x221 = x105*x69;
                    let x222 = x101*x68;
                    //final jacobian: (shape: 2 x 6)
                    j[(0,0)] = -fx*(p2*(three*x109 + three*x110 + x113) + x100*x98 + x106*x48 + x107*x108 + x116*x117 + x48*x71 + x84*x90);
                    j[(0,1)] = -fx*(p2*(three*x149 + three*x150 + x153) + x100*x147 + x106*x142 + x108*x148 + x117*x155 + x132*x90 + x142*x71);
                    j[(0,2)] = -fx*(p2*(three*x174 + three*x175 + x178) + x100*x172 + x106*x168 + x108*x173 + x117*x180 + x163*x90 + x168*x71);
                    j[(0,3)] = -fx*(p2*(three*x185 + three*x188 + x193) + x100*x184 + x106*x87 + x108*x55 + x117*x195 + x181*x90 + x71*x87);
                    j[(0,4)] = -fx*(p2*(three*x200 + three*x201 + x205) + x100*x199 + x106*x196 + x108*x52 + x117*x207 + x196*x71 + x67*x90);
                    j[(0,5)] = -fx*(p2*(three*x210 + three*x211 + x214) + x100*x209 + x106*x208 + x108*x59 + x117*x216 + x208*x71 + x65*x90);
                    j[(1,0)] = -fy*(p1*(three*x111 + three*x112 + x114) + x106*x84 + x107*x221 + x116*x222 + x218*x48 + x219*x84 + x220*x98);
                    j[(1,1)] = -fy*(p1*(three*x151 + three*x152 + x154) + x106*x132 + x132*x219 + x142*x218 + x147*x220 + x148*x221 + x155*x222);
                    j[(1,2)] = -fy*(p1*(three*x176 + three*x177 + x179) + x106*x163 + x163*x219 + x168*x218 + x172*x220 + x173*x221 + x180*x222);
                    j[(1,3)] = -fy*(p1*(three*x189 + three*x192 + x194) + x106*x181 + x181*x219 + x184*x220 + x195*x222 + x218*x87 + x221*x55);
                    j[(1,4)] = -fy*(p1*(three*x202 + three*x204 + x206) + x106*x67 + x196*x218 + x199*x220 + x207*x222 + x219*x67 + x221*x52);
                    j[(1,5)] = -fy*(p1*(three*x212 + three*x213 + x215) + x106*x65 + x208*x218 + x209*x220 + x216*x222 + x219*x65 + x221*x59);
                }
            }
        }
    }
}
