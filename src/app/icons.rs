//! Vector icon set in the style of Creo's ribbon: yellow shaded solids, tan datums,
//! blue sketch geometry and arrows. Icons are drawn on a 16×16 grid and scale to any size.

use eframe::egui::{self, pos2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Icon {
    New, Open, Save, SaveAs, Undo, Redo, Regenerate, Export, Close, Exit, Options, Help, Recent,
    Plane, Axis, Point, Csys, Sketch, Extrude, Revolve, Sweep, Blend, Hole, Round, Chamfer, Shell, Draft, Pattern, Mirror,
    PatternFill, PatternCurve,
    Parameters, Relations, Measure, MassProps, ModelInfo, Player,
    Refit, ZoomIn, ZoomOut, Repaint, SavedViews, DispShadedEdges, DispShaded, DispHidden, DispNoHidden, DispWire,
    DatumDisplay, SpinCenter, Section, Perspective, Appearance, Transparent,
    SkSelect, SkLine, SkRect, SkCenterRect, SkCircle, SkCircle3, SkArc3, SkArcCenter, SkFillet, SkChamfer, SkCenterline,
    SkPoint, SkPalette, SkSpline, SkEllipse, SkDeleteSeg, SkConstruction, SkDimension, SkModify, SkMirror, SkDivide, SkCorner, SkProject,
    ConV, ConH, ConPerp, ConTan, ConMid, ConCoinc, ConSym, ConEqual, ConPar,
    SketchView, Requirements, OpenEnds, ShadeLoops, Grid, Vertex, DimDisplay, ConDisplay,
    Ok, Cancel, Preview, Flip, RemoveMaterial, Thicken, DepthBlind, DepthSymmetric, DepthThrough, DepthNext, DepthSelected, Taper,
    HoleSimple, HoleCbore, HoleCsink, HoleStandard,
    Delete, Suppress, Resume, Hide, Show, EditDef, EditDims,
    Part, InsertHere, Failed, TreeTab, FolderTab, FolderItem, FileItem, Up,
    RegenOk, RegenFail,
    Assembly, Assemble, CreateComp, Explode, Bom, Interference, ReturnAsm, DragComp,
    AsmAuto, AsmDistance, AsmAngle, AsmParallel, AsmCoincident, AsmNormal, AsmDefault, AsmFix,
    Drawing, Sheet, NewSheet, GeneralView, ProjView, DetailView, SectionView, DwgDim, Note, Balloon, ShowAnnot,
    TitleBlock, Pdf, Dxf, Svg, Cleanup, Lock, ReturnDwg,
    GeomTol, DatumFeature, SurfFinish, DimTol,
    Copy, Paste, GetData, Body, Rib, SweptBlend, ShortEdge, Thickness, GeomReport, DraftCheck, MeshSurf, DevAngle, Curvature,
    PairClear, DimBound, Probe, SimReport, FieldOfView, Contact, Fixed, Displacement, Force, Moment, Pressure, Gravity,
    Centrifugal, LinAccel, Deform, ComboState, AnnotPlane, AnnotFeature, DatumTarget, RefViewer, Search, CompInterface,
    PublishGeom, FamilyTable, SwitchDims, UdfLib, ImageEditor, Hatch, AuxApps, ArTarget, Layers, Scene, Pan, ZoomSel,
    ViewBack, Windows, ActivateWin, FlexMove, FlexOffset, FlexModify, FlexSymmetry, FlexRounds, Islands, Weld, Mold,
    Moldbase, GenDesign, Simulate, SimResults, Flow, MoldFlow, Mathcad, GdtAdvisor, EzTol, Render, BoundaryBlend,
    FillSurf, Style, Freestyle, Extend, Trim, Merge, Intersect, Offset, Solidify, Project3d, WorkDir, Erase, Server,
    SysColors, ModelDisplay, Trail, LastSession, FullScreen, Globe, BgWhite, BgStd, BgDark, Pdf3d, ImportParams,
    MassCalc, Note3d, Sheetmetal, Collapse, ExpandAll, Columns, Filter, Settings, PlaneTags, Customize,
    HelicalSweep, RotBlend, CosmThread, CurvePoints,
    DispReflect, Reorient, NormalView, SkCheck, SkLock, HlOverlap, HlIntersect, HlBranch,
    SkLineTangent, SkCenterlineTangent, SkSlantRect, SkParallelogram, SkCircleConc, SkCircle3T, SkArc3T, SkArcConc,
    SkConic, SkEllipseAxis, SkFilletTrim, SkFilletEllip, SkChamferTrim, SelChain, SelAll, SkReplace, SkConvert,
    InfoIntersect, InfoTangent, InfoEntity,
    SketchSetup, References, Cut, SkText, SkCsys, SkOffset, SkThicken, RotResize, RefDim, SkImport, Perimeter, Baseline,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 { Color32::from_rgb(r, g, b) }

const YT: Color32 = rgb(252, 230, 142);
const YL: Color32 = rgb(240, 192, 82);
const YR: Color32 = rgb(214, 150, 42);
const YO: Color32 = rgb(140, 98, 22);
const BLUE: Color32 = rgb(38, 104, 198);
const BLUE_L: Color32 = rgb(150, 190, 240);
const SK: Color32 = rgb(30, 96, 220);
const TAN: Color32 = rgb(236, 220, 178);
const BROWN: Color32 = rgb(150, 110, 52);
const GREEN: Color32 = rgb(48, 160, 60);
const GREEN_D: Color32 = rgb(28, 112, 38);
const RED: Color32 = rgb(214, 56, 44);
const DARK: Color32 = rgb(60, 66, 74);
const MID: Color32 = rgb(126, 134, 144);
const LIGHT: Color32 = rgb(214, 219, 225);
const WHITE: Color32 = Color32::WHITE;
const GTOP: Color32 = rgb(232, 236, 240);
const GLEFT: Color32 = rgb(196, 203, 211);
const GRIGHT: Color32 = rgb(160, 169, 179);
const GLINE: Color32 = rgb(70, 78, 88);

type P = (f32, f32);

struct Ic<'a> {
    p: &'a Painter,
    o: Pos2,
    s: f32,
    en: bool,
}

impl Ic<'_> {
    fn pt(&self, (x, y): P) -> Pos2 { pos2(self.o.x + x * self.s, self.o.y + y * self.s) }
    fn c(&self, c: Color32) -> Color32 {
        if self.en { return c; }
        let l = 0.3 * c.r() as f32 + 0.59 * c.g() as f32 + 0.11 * c.b() as f32;
        let g = (l * 0.35 + 150.0).min(235.0) as u8;
        Color32::from_rgba_unmultiplied(g, g, g, c.a())
    }
    fn w(&self, w: f32) -> f32 { (w * self.s * 0.72).max(1.0) }
    fn poly(&self, pts: &[P], fill: Color32, line: Option<Color32>) {
        let v: Vec<Pos2> = pts.iter().map(|&q| self.pt(q)).collect();
        let st = line.map(|l| Stroke::new(self.w(0.9), self.c(l))).unwrap_or(Stroke::NONE);
        self.p.add(Shape::convex_polygon(v, self.c(fill), st));
    }
    fn path(&self, pts: &[P], w: f32, col: Color32) {
        let v: Vec<Pos2> = pts.iter().map(|&q| self.pt(q)).collect();
        self.p.add(Shape::line(v, Stroke::new(self.w(w), self.c(col))));
    }
    fn closed(&self, pts: &[P], w: f32, col: Color32) {
        let v: Vec<Pos2> = pts.iter().map(|&q| self.pt(q)).collect();
        self.p.add(Shape::closed_line(v, Stroke::new(self.w(w), self.c(col))));
    }
    fn dashed(&self, a: P, b: P, w: f32, col: Color32, dash: f32, gap: f32) {
        self.p.extend(Shape::dashed_line(&[self.pt(a), self.pt(b)], Stroke::new(self.w(w), self.c(col)), dash * self.s, gap * self.s));
    }
    fn circle(&self, c: P, r: f32, fill: Color32, line: Option<(f32, Color32)>) {
        let st = line.map(|(w, l)| Stroke::new(self.w(w), self.c(l))).unwrap_or(Stroke::NONE);
        self.p.circle(self.pt(c), r * self.s, self.c(fill), st);
    }
    fn ring(&self, c: P, r: f32, w: f32, col: Color32) { self.p.circle_stroke(self.pt(c), r * self.s, Stroke::new(self.w(w), self.c(col))); }
    fn arc(&self, c: P, rx: f32, ry: f32, a0: f32, a1: f32) -> Vec<P> {
        let n = 20;
        (0..=n).map(|i| {
            let t = (a0 + (a1 - a0) * i as f32 / n as f32).to_radians();
            (c.0 + rx * t.cos(), c.1 - ry * t.sin())
        }).collect()
    }
    fn head(&self, tip: P, from: P, size: f32, col: Color32) {
        let (dx, dy) = (tip.0 - from.0, tip.1 - from.1);
        let l = (dx * dx + dy * dy).sqrt().max(1e-6);
        let (ux, uy) = (dx / l, dy / l);
        let (nx, ny) = (-uy, ux);
        let b = (tip.0 - ux * size, tip.1 - uy * size);
        self.poly(&[tip, (b.0 + nx * size * 0.55, b.1 + ny * size * 0.55), (b.0 - nx * size * 0.55, b.1 - ny * size * 0.55)], col, None);
    }
    fn arrow(&self, from: P, to: P, w: f32, col: Color32, size: f32) {
        let (dx, dy) = (to.0 - from.0, to.1 - from.1);
        let l = (dx * dx + dy * dy).sqrt().max(1e-6);
        let end = (to.0 - dx / l * size * 0.7, to.1 - dy / l * size * 0.7);
        self.path(&[from, end], w, col);
        self.head(to, from, size, col);
    }
    fn sq(&self, c: P, r: f32, col: Color32) {
        self.p.rect_filled(Rect::from_center_size(self.pt(c), egui::vec2(2.0 * r * self.s, 2.0 * r * self.s)), 0.0, self.c(col));
    }
    fn text(&self, c: P, t: &str, size: f32, col: Color32) {
        self.p.text(self.pt(c), egui::Align2::CENTER_CENTER, t, FontId::proportional(size * self.s), self.c(col));
    }
    /// Isometric box: `o` is the front-bottom corner, `a` along +X (right), `b` along +Z (left), `h` up.
    fn iso_pal(&self, o: P, a: f32, b: f32, h: f32, pal: [Color32; 4]) {
        let x = (0.866 * a, -0.5 * a);
        let z = (-0.866 * b, -0.5 * b);
        let ox = (o.0 + x.0, o.1 + x.1);
        let oz = (o.0 + z.0, o.1 + z.1);
        let oxz = (ox.0 + z.0, ox.1 + z.1);
        let up = |q: P| (q.0, q.1 - h);
        self.poly(&[o, ox, up(ox), up(o)], pal[2], Some(pal[3]));
        self.poly(&[oz, o, up(o), up(oz)], pal[1], Some(pal[3]));
        self.poly(&[up(o), up(ox), up(oxz), up(oz)], pal[0], Some(pal[3]));
    }
    fn iso(&self, o: P, a: f32, b: f32, h: f32) { self.iso_pal(o, a, b, h, [YT, YL, YR, YO]); }
    fn iso_top_center(o: P, a: f32, b: f32, h: f32) -> P {
        (o.0 + 0.866 * (a - b) * 0.5, o.1 - 0.5 * (a + b) * 0.5 - h)
    }
    fn plane(&self, pts: [P; 4]) { self.poly(&pts, TAN, Some(BROWN)); }
    fn magnifier(&self, sign: Option<bool>) {
        self.path(&[(10.0, 10.0), (14.5, 14.5)], 2.4, DARK);
        self.circle((7.0, 7.0), 5.0, rgb(226, 238, 252), Some((1.4, DARK)));
        match sign {
            Some(true) => { self.path(&[(4.5, 7.0), (9.5, 7.0)], 1.5, BLUE); self.path(&[(7.0, 4.5), (7.0, 9.5)], 1.5, BLUE); }
            Some(false) => self.path(&[(4.5, 7.0), (9.5, 7.0)], 1.5, BLUE),
            None => {}
        }
    }
    fn pencil(&self, tip: P) {
        let (x, y) = tip;
        self.poly(&[(x, y), (x + 1.2, y - 2.6), (x + 2.6, y - 1.2)], rgb(240, 210, 170), Some(DARK));
        self.poly(&[(x + 1.2, y - 2.6), (x + 6.0, y - 7.4), (x + 7.4, y - 6.0), (x + 2.6, y - 1.2)], rgb(246, 196, 60), Some(DARK));
    }
    fn eye(&self) {
        let up = self.arc((8.0, 11.5), 6.5, 6.0, 25.0, 155.0);
        let lo = self.arc((8.0, 4.5), 6.5, 6.0, 205.0, 335.0);
        self.path(&up, 1.2, DARK);
        self.path(&lo, 1.2, DARK);
        self.circle((8.0, 8.0), 2.4, BLUE, Some((0.8, DARK)));
    }
    fn hole_section(&self, kind: u8) {
        self.poly(&[(1.5, 4.0), (5.5, 4.0), (5.5, 14.0), (1.5, 14.0)], GLEFT, Some(GLINE));
        self.poly(&[(10.5, 4.0), (14.5, 4.0), (14.5, 14.0), (10.5, 14.0)], GLEFT, Some(GLINE));
        match kind {
            1 => {
                self.poly(&[(3.5, 4.0), (5.5, 4.0), (5.5, 7.0), (3.5, 7.0)], WHITE, None);
                self.poly(&[(10.5, 4.0), (12.5, 4.0), (12.5, 7.0), (10.5, 7.0)], WHITE, None);
                self.path(&[(3.5, 4.0), (3.5, 7.0), (5.5, 7.0), (5.5, 14.0)], 1.0, GLINE);
                self.path(&[(12.5, 4.0), (12.5, 7.0), (10.5, 7.0), (10.5, 14.0)], 1.0, GLINE);
            }
            2 => {
                self.poly(&[(3.0, 4.0), (5.5, 4.0), (5.5, 6.5)], WHITE, None);
                self.poly(&[(10.5, 4.0), (13.0, 4.0), (10.5, 6.5)], WHITE, None);
                self.path(&[(3.0, 4.0), (5.5, 6.5), (5.5, 14.0)], 1.0, GLINE);
                self.path(&[(13.0, 4.0), (10.5, 6.5), (10.5, 14.0)], 1.0, GLINE);
            }
            _ => {}
        }
        self.dashed((8.0, 2.0), (8.0, 15.5), 0.8, MID, 2.0, 1.2);
    }
}

/// Paint `icon` centred in `rect`.
pub fn paint(p: &Painter, rect: Rect, icon: Icon, enabled: bool) {
    let side = rect.width().min(rect.height());
    let o = rect.center() - egui::vec2(side / 2.0, side / 2.0);
    let k = Ic { p, o, s: side / 16.0, en: enabled };
    use Icon::*;
    match icon {
        New => {
            k.poly(&[(3.0, 1.5), (10.5, 1.5), (13.0, 4.0), (13.0, 14.5), (3.0, 14.5)], WHITE, Some(MID));
            k.poly(&[(10.5, 1.5), (10.5, 4.0), (13.0, 4.0)], LIGHT, Some(MID));
            k.poly(&[(5.5, 4.5), (6.3, 6.7), (5.5, 8.9), (4.7, 6.7)], rgb(250, 190, 40), None);
            k.poly(&[(3.3, 6.7), (5.5, 5.9), (7.7, 6.7), (5.5, 7.5)], rgb(250, 190, 40), None);
        }
        Open | FolderItem | FolderTab => {
            k.poly(&[(1.5, 3.5), (6.0, 3.5), (7.2, 5.0), (14.0, 5.0), (14.0, 13.0), (1.5, 13.0)], rgb(226, 170, 50), Some(rgb(150, 105, 20)));
            k.poly(&[(3.2, 7.0), (15.2, 7.0), (13.6, 13.0), (1.5, 13.0)], rgb(250, 212, 110), Some(rgb(150, 105, 20)));
            if icon == Open { k.arrow((10.0, 9.5), (10.0, 1.5), 1.4, GREEN, 3.0); }
        }
        Save | SaveAs => {
            k.poly(&[(2.0, 2.0), (12.5, 2.0), (14.0, 3.5), (14.0, 14.0), (2.0, 14.0)], rgb(58, 110, 196), Some(rgb(28, 66, 136)));
            k.poly(&[(4.5, 2.0), (11.5, 2.0), (11.5, 6.5), (4.5, 6.5)], rgb(228, 236, 247), None);
            k.poly(&[(9.0, 2.6), (10.6, 2.6), (10.6, 5.8), (9.0, 5.8)], rgb(58, 110, 196), None);
            k.poly(&[(4.0, 9.0), (12.0, 9.0), (12.0, 14.0), (4.0, 14.0)], WHITE, None);
            if icon == SaveAs { k.pencil((8.5, 14.5)); }
        }
        Undo | Redo => {
            let flip = |q: P| if icon == Redo { (16.0 - q.0, q.1) } else { q };
            let pts: Vec<P> = k.arc((8.6, 11.0), 5.0, 5.0, 0.0, 165.0).into_iter().map(flip).collect();
            k.path(&pts, 2.0, BLUE);
            let tip = flip((3.4, 12.6));
            let from = flip((3.6, 9.0));
            k.head(tip, from, 4.0, BLUE);
        }
        Regenerate => {
            k.iso((8.0, 12.5), 3.0, 3.0, 3.0);
            let a1 = k.arc((8.0, 8.0), 6.6, 6.6, 30.0, 150.0);
            let a2 = k.arc((8.0, 8.0), 6.6, 6.6, 210.0, 330.0);
            k.path(&a1, 1.6, GREEN);
            k.path(&a2, 1.6, GREEN);
            k.head(a1[a1.len() - 1], a1[a1.len() - 4], 3.2, GREEN);
            k.head(a2[a2.len() - 1], a2[a2.len() - 4], 3.2, GREEN);
        }
        Export => {
            k.iso((7.0, 14.5), 4.0, 4.0, 4.0);
            k.arrow((8.0, 6.0), (15.0, 1.5), 1.6, GREEN, 3.4);
        }
        Close | Exit => {
            k.poly(&[(2.0, 2.5), (14.0, 2.5), (14.0, 13.5), (2.0, 13.5)], WHITE, Some(MID));
            k.poly(&[(2.0, 2.5), (14.0, 2.5), (14.0, 5.0), (2.0, 5.0)], rgb(58, 110, 196), None);
            k.path(&[(5.5, 7.0), (10.5, 12.0)], 1.8, RED);
            k.path(&[(10.5, 7.0), (5.5, 12.0)], 1.8, RED);
        }
        Options => {
            for i in 0..8 {
                let t = (i as f32 * 45.0f32).to_radians();
                k.path(&[(8.0 + 4.5 * t.cos(), 8.0 + 4.5 * t.sin()), (8.0 + 6.8 * t.cos(), 8.0 + 6.8 * t.sin())], 2.4, MID);
            }
            k.circle((8.0, 8.0), 5.0, LIGHT, Some((1.2, DARK)));
            k.circle((8.0, 8.0), 2.0, WHITE, Some((1.0, DARK)));
        }
        Help => { k.circle((8.0, 8.0), 6.8, BLUE, None); k.text((8.0, 8.3), "?", 11.0, WHITE); }
        Recent => {
            k.circle((8.0, 8.0), 6.5, WHITE, Some((1.3, DARK)));
            k.path(&[(8.0, 3.8), (8.0, 8.0), (11.0, 10.0)], 1.4, BLUE);
        }
        Plane => k.plane([(1.0, 12.0), (5.0, 4.0), (15.0, 4.0), (11.0, 12.0)]),
        Axis => {
            k.poly(&[(1.5, 12.5), (4.5, 6.5), (13.5, 6.5), (10.5, 12.5)], rgb(244, 236, 214), Some(rgb(200, 178, 130)));
            k.dashed((3.0, 15.0), (13.0, 1.0), 1.5, BROWN, 3.0, 1.2);
        }
        Point => {
            k.poly(&[(1.5, 13.0), (4.5, 7.0), (14.0, 7.0), (11.0, 13.0)], rgb(244, 236, 214), Some(rgb(200, 178, 130)));
            k.path(&[(5.5, 3.5), (10.5, 8.5)], 1.6, BROWN);
            k.path(&[(10.5, 3.5), (5.5, 8.5)], 1.6, BROWN);
        }
        Csys => {
            let o0 = (6.0, 10.5);
            k.arrow(o0, (14.5, 13.0), 1.2, BROWN, 3.0);
            k.arrow(o0, (6.0, 1.5), 1.2, BROWN, 3.0);
            k.arrow(o0, (1.5, 14.5), 1.2, BROWN, 3.0);
        }
        Sketch => {
            k.plane([(1.0, 13.5), (4.5, 6.5), (15.0, 6.5), (11.5, 13.5)]);
            let c: Vec<P> = (0..=16).map(|i| { let t = i as f32 / 16.0; (3.5 + 9.0 * t, 11.2 - 2.6 * (t * 6.28).sin() * 0.6 - t * 1.2) }).collect();
            k.path(&c, 1.5, SK);
            k.pencil((9.0, 7.0));
        }
        Extrude => {
            k.iso((8.0, 15.0), 5.0, 5.0, 4.0);
            k.arrow((8.0, 8.6), (8.0, 1.0), 1.8, BLUE, 3.6);
        }
        Revolve => {
            k.dashed((8.0, 0.8), (8.0, 15.5), 0.9, DARK, 2.4, 1.2);
            k.poly(&[(3.5, 5.0), (12.5, 5.0), (12.5, 12.0), (3.5, 12.0)], YL, None);
            k.poly(&[(8.0, 5.0), (12.5, 5.0), (12.5, 12.0), (8.0, 12.0)], YR, None);
            let bot: Vec<P> = k.arc((8.0, 12.0), 4.5, 1.6, 180.0, 360.0);
            let mut b2 = bot.clone();
            b2.push((8.0, 12.0));
            k.poly(&b2, YR, None);
            k.path(&bot, 0.9, YO);
            let top: Vec<P> = k.arc((8.0, 5.0), 4.5, 1.6, 0.0, 360.0);
            k.poly(&top, YT, Some(YO));
            k.path(&[(3.5, 5.0), (3.5, 12.0)], 0.9, YO);
            k.path(&[(12.5, 5.0), (12.5, 12.0)], 0.9, YO);
            let ar = k.arc((8.0, 9.0), 7.0, 2.6, 200.0, 340.0);
            k.path(&ar, 1.5, BLUE);
            k.head(ar[ar.len() - 1], ar[ar.len() - 3], 3.0, BLUE);
        }
        Sweep => {
            let c: Vec<P> = (0..=20).map(|i| { let t = i as f32 / 20.0; (2.0 + 11.0 * t, 13.0 - 9.0 * t + 3.0 * (t * 3.14).sin()) }).collect();
            k.path(&c, 4.6, YO);
            k.path(&c, 3.4, YL);
            let hi: Vec<P> = c.iter().map(|q| (q.0 - 0.4, q.1 - 0.6)).collect();
            k.path(&hi, 1.0, YT);
            k.circle(c[c.len() - 1], 2.3, YT, Some((0.9, YO)));
        }
        Blend => {
            let (bf, br, bb, bl) = ((8.0, 15.0), (13.5, 12.0), (8.0, 9.0), (2.5, 12.0));
            let (tf, tr, tb, tl) = ((8.0, 7.0), (10.5, 5.6), (8.0, 4.2), (5.5, 5.6));
            let _ = bb;
            k.poly(&[bl, bf, tf, tl], YL, Some(YO));
            k.poly(&[bf, br, tr, tf], YR, Some(YO));
            k.poly(&[tf, tr, tb, tl], YT, Some(YO));
        }
        Hole => {
            k.iso((8.0, 14.5), 5.6, 5.6, 3.0);
            let c = Ic::iso_top_center((8.0, 14.5), 5.6, 5.6, 3.0);
            let e = k.arc(c, 2.6, 1.4, 0.0, 360.0);
            k.poly(&e, rgb(92, 64, 12), Some(YO));
            let inner = k.arc((c.0, c.1 + 0.4), 2.0, 0.9, 180.0, 360.0);
            k.path(&inner, 0.8, YR);
        }
        Round => {
            k.iso((8.0, 15.0), 5.0, 5.0, 6.0);
            k.poly(&[(7.0, 9.4), (9.0, 9.4), (9.0, 15.0), (7.0, 15.0)], rgb(255, 244, 196), None);
            k.path(&[(7.0, 9.4), (7.0, 14.4)], 0.8, YO);
            k.path(&[(9.0, 9.4), (9.0, 14.4)], 0.8, YO);
            k.path(&k.arc((8.0, 9.4), 1.0, 0.5, 0.0, 180.0), 0.9, BLUE);
        }
        Chamfer => {
            k.iso((8.0, 15.0), 5.0, 5.0, 6.0);
            k.poly(&[(6.4, 8.2), (9.6, 8.2), (9.6, 15.2), (6.4, 15.2)], rgb(248, 214, 120), Some(YO));
            k.path(&[(6.4, 8.2), (9.6, 8.2)], 1.2, BLUE);
        }
        Shell => {
            k.iso((8.0, 15.0), 5.0, 5.0, 5.0);
            let c = Ic::iso_top_center((8.0, 15.0), 5.0, 5.0, 5.0);
            let sc = 0.62;
            let pts = [(0.0, 2.5), (4.33, 0.0), (0.0, -2.5), (-4.33, 0.0)];
            let inner: Vec<P> = pts.iter().map(|q| (c.0 + q.0 * sc, c.1 + q.1 * sc)).collect();
            k.poly(&inner, rgb(176, 120, 30), Some(YO));
            k.poly(&[inner[3], inner[0], (inner[0].0, inner[0].1 - 0.0), (inner[3].0 + 0.4, inner[3].1 + 1.6)], YR, None);
        }
        Draft => {
            k.poly(&[(2.5, 14.0), (8.0, 15.5), (8.5, 7.0), (5.0, 6.0)], YL, Some(YO));
            k.poly(&[(8.0, 15.5), (13.5, 14.0), (11.0, 6.0), (8.5, 7.0)], YR, Some(YO));
            k.poly(&[(5.0, 6.0), (8.5, 7.0), (11.0, 6.0), (7.6, 5.0)], YT, Some(YO));
            let a = k.arc((13.5, 14.0), 4.0, 4.0, 100.0, 125.0);
            k.path(&a, 1.0, BLUE);
        }
        Pattern => {
            for (x, y) in [(4.5, 8.0), (11.5, 8.0), (4.5, 15.0), (11.5, 15.0)] { k.iso((x, y), 2.6, 2.6, 2.4); }
        }
        PatternFill => {
            k.poly(&[(1.0, 9.0), (8.0, 4.5), (15.0, 9.0), (8.0, 13.5)], rgb(236, 220, 178), Some(BROWN));
            for (x, y) in [(6.0, 9.0), (10.0, 9.0), (8.0, 7.6), (8.0, 10.6)] { k.iso((x, y), 1.4, 1.4, 2.0); }
        }
        PatternCurve => {
            let c: Vec<P> = (0..=20).map(|i| { let t = i as f32 / 20.0; (1.5 + 13.0 * t, 13.0 - 9.0 * t + 3.5 * (t * 3.14).sin()) }).collect();
            k.path(&c, 1.2, SK);
            for idx in [2usize, 10, 18] { let q = c[idx]; k.iso((q.0, q.1 + 1.5), 1.6, 1.6, 2.4); }
        }
        Mirror => {
            k.poly(&[(7.0, 2.4), (9.0, 0.8), (9.0, 13.8), (7.0, 15.4)], rgb(236, 220, 178), Some(BROWN));
            k.iso((3.8, 13.5), 2.4, 2.4, 4.0);
            k.iso_pal((12.4, 13.5), 2.4, 2.4, 4.0, [rgb(253, 240, 196), rgb(247, 218, 150), rgb(232, 196, 120), rgb(190, 150, 70)]);
        }
        Parameters => {
            k.poly(&[(1.5, 3.0), (14.5, 3.0), (14.5, 13.5), (1.5, 13.5)], WHITE, Some(MID));
            k.poly(&[(1.5, 3.0), (14.5, 3.0), (14.5, 5.6), (1.5, 5.6)], rgb(58, 110, 196), None);
            for y in [8.2, 10.8] { k.path(&[(1.5, y), (14.5, y)], 0.6, LIGHT); }
            k.path(&[(6.0, 5.6), (6.0, 13.5)], 0.6, LIGHT);
            k.text((10.3, 9.6), "[ ]", 5.0, DARK);
        }
        Relations => { k.text((8.0, 8.5), "d=", 9.5, rgb(30, 60, 140)); k.path(&[(2.0, 14.0), (14.0, 14.0)], 0.8, MID); }
        Measure => {
            k.poly(&[(1.0, 9.5), (15.0, 9.5), (15.0, 14.0), (1.0, 14.0)], rgb(244, 214, 110), Some(YO));
            for i in 0..7 { let x = 2.5 + i as f32 * 2.0; k.path(&[(x, 9.5), (x, if i % 2 == 0 { 12.0 } else { 11.0 })], 0.7, YO); }
            k.path(&[(2.0, 3.0), (2.0, 7.5)], 0.7, BLUE);
            k.path(&[(14.0, 3.0), (14.0, 7.5)], 0.7, BLUE);
            k.arrow((8.0, 5.0), (2.2, 5.0), 1.0, BLUE, 2.4);
            k.arrow((8.0, 5.0), (13.8, 5.0), 1.0, BLUE, 2.4);
        }
        MassProps => {
            k.iso((7.0, 15.0), 4.2, 4.2, 4.5);
            k.text((12.0, 4.0), "kg", 6.5, rgb(30, 60, 140));
        }
        ModelInfo => { k.circle((8.0, 8.0), 6.8, BLUE, None); k.text((8.0, 8.4), "i", 10.0, WHITE); }
        Player => {
            k.poly(&[(1.5, 3.0), (14.5, 3.0), (14.5, 13.0), (1.5, 13.0)], LIGHT, Some(MID));
            k.poly(&[(6.0, 5.5), (11.0, 8.0), (6.0, 10.5)], GREEN, None);
        }
        Refit => {
            k.dashed((2.0, 2.0), (14.0, 2.0), 1.0, MID, 2.0, 1.2);
            k.dashed((14.0, 2.0), (14.0, 14.0), 1.0, MID, 2.0, 1.2);
            k.dashed((14.0, 14.0), (2.0, 14.0), 1.0, MID, 2.0, 1.2);
            k.dashed((2.0, 14.0), (2.0, 2.0), 1.0, MID, 2.0, 1.2);
            k.iso((8.0, 13.0), 3.6, 3.6, 3.6);
        }
        ZoomIn => k.magnifier(Some(true)),
        ZoomOut => k.magnifier(Some(false)),
        Repaint => {
            k.path(&[(9.0, 7.0), (14.0, 2.0)], 2.2, rgb(150, 100, 40));
            k.poly(&[(7.0, 6.0), (10.0, 9.0), (8.8, 10.2), (5.8, 7.2)], MID, Some(DARK));
            k.poly(&[(5.8, 7.2), (8.8, 10.2), (4.0, 15.0), (1.5, 12.5)], rgb(60, 140, 220), Some(DARK));
        }
        SavedViews => {
            k.iso_pal((8.0, 15.0), 5.0, 5.0, 5.0, [WHITE, rgb(150, 190, 240), WHITE, DARK]);
            k.circle((12.5, 3.5), 2.6, WHITE, Some((1.0, DARK)));
            k.circle((12.5, 3.5), 1.0, BLUE, None);
        }
        DispShadedEdges => k.iso_pal((8.0, 15.0), 5.2, 5.2, 5.2, [GTOP, GLEFT, GRIGHT, rgb(20, 20, 24)]),
        DispShaded => k.iso_pal((8.0, 15.0), 5.2, 5.2, 5.2, [GTOP, GLEFT, GRIGHT, GRIGHT]),
        DispHidden | DispNoHidden | DispWire => {
            let o0 = (8.0, 15.0);
            let (a, h) = (5.2, 5.2);
            let x = (0.866 * a, -0.5 * a);
            let z = (-0.866 * a, -0.5 * a);
            let ox = (o0.0 + x.0, o0.1 + x.1);
            let oz = (o0.0 + z.0, o0.1 + z.1);
            let oxz = (ox.0 + z.0, ox.1 + z.1);
            let up = |q: P| (q.0, q.1 - h);
            if icon != DispWire { k.poly(&[o0, ox, up(ox), up(o0)], WHITE, None); k.poly(&[oz, o0, up(o0), up(oz)], WHITE, None); k.poly(&[up(o0), up(ox), up(oxz), up(oz)], WHITE, None); }
            for (a1, b1) in [(o0, ox), (o0, oz), (o0, up(o0)), (ox, up(ox)), (oz, up(oz)), (up(o0), up(ox)), (up(o0), up(oz)), (up(ox), up(oxz)), (up(oz), up(oxz))] { k.path(&[a1, b1], 1.0, DARK); }
            match icon {
                DispHidden => { k.dashed(oxz, up(oxz), 0.8, MID, 1.2, 1.0); k.dashed(oxz, ox, 0.8, MID, 1.2, 1.0); k.dashed(oxz, oz, 0.8, MID, 1.2, 1.0); }
                DispWire => { k.path(&[oxz, up(oxz)], 1.0, DARK); k.path(&[oxz, ox], 1.0, DARK); k.path(&[oxz, oz], 1.0, DARK); }
                _ => {}
            }
        }
        DatumDisplay => {
            k.plane([(1.0, 11.0), (4.0, 5.0), (12.0, 5.0), (9.0, 11.0)]);
            k.dashed((12.5, 15.0), (12.5, 1.0), 1.2, BROWN, 2.2, 1.0);
            k.path(&[(2.0, 13.0), (5.0, 16.0)], 1.2, BROWN);
            k.path(&[(5.0, 13.0), (2.0, 16.0)], 1.2, BROWN);
        }
        SpinCenter => {
            let c = (8.0, 9.0);
            k.path(&[c, (14.5, 12.5)], 1.6, RED);
            k.path(&[c, (8.0, 1.5)], 1.6, GREEN);
            k.path(&[c, (1.5, 12.5)], 1.6, BLUE);
            k.circle(c, 1.4, DARK, None);
        }
        Section => {
            k.iso((8.0, 15.0), 5.0, 5.0, 5.0);
            k.poly(&[(8.0, 15.0), (12.33, 12.5), (12.33, 7.5), (8.0, 10.0)], rgb(232, 120, 80), Some(rgb(150, 60, 30)));
            for i in 0..3 { let t = i as f32 * 1.4; k.path(&[(8.6 + t, 13.8 - t * 0.6), (8.6 + t, 10.6 - t * 0.6)], 0.6, rgb(150, 60, 30)); }
        }
        Perspective => {
            k.closed(&[(5.5, 5.5), (10.5, 5.5), (10.5, 10.5), (5.5, 10.5)], 1.0, DARK);
            k.closed(&[(1.0, 1.0), (15.0, 1.0), (15.0, 15.0), (1.0, 15.0)], 1.0, MID);
            for (a1, b1) in [((1.0, 1.0), (5.5, 5.5)), ((15.0, 1.0), (10.5, 5.5)), ((15.0, 15.0), (10.5, 10.5)), ((1.0, 15.0), (5.5, 10.5))] { k.path(&[a1, b1], 0.8, MID); }
        }
        Appearance => {
            k.circle((8.0, 8.5), 6.5, rgb(66, 130, 214), Some((0.8, rgb(30, 70, 140))));
            k.circle((6.0, 6.2), 2.4, rgb(170, 205, 245), None);
            k.circle((5.4, 5.6), 1.0, WHITE, None);
        }
        Transparent => {
            k.iso_pal((8.0, 15.0), 5.2, 5.2, 5.2, [rgb(230, 238, 248), rgb(214, 226, 242), rgb(196, 212, 236), rgb(120, 140, 170)]);
        }
        SkSelect => {
            k.poly(&[(4.0, 1.5), (4.0, 13.5), (7.0, 10.6), (9.4, 15.0), (11.2, 14.0), (8.8, 9.8), (12.6, 9.6)], WHITE, Some(DARK));
        }
        SkLine => { k.path(&[(3.0, 13.0), (13.0, 3.0)], 1.6, SK); k.sq((3.0, 13.0), 1.2, DARK); k.sq((13.0, 3.0), 1.2, DARK); }
        SkRect => { k.closed(&[(2.5, 4.0), (13.5, 4.0), (13.5, 12.0), (2.5, 12.0)], 1.5, SK); for q in [(2.5, 4.0), (13.5, 4.0), (13.5, 12.0), (2.5, 12.0)] { k.sq(q, 1.1, DARK); } }
        SkCenterRect => {
            k.closed(&[(2.5, 4.0), (13.5, 4.0), (13.5, 12.0), (2.5, 12.0)], 1.5, SK);
            k.dashed((2.5, 4.0), (13.5, 12.0), 0.7, BLUE_L, 1.5, 1.0);
            k.dashed((13.5, 4.0), (2.5, 12.0), 0.7, BLUE_L, 1.5, 1.0);
            k.sq((8.0, 8.0), 1.2, DARK);
        }
        SkCircle => { k.ring((8.0, 8.0), 5.8, 1.5, SK); k.path(&[(6.8, 8.0), (9.2, 8.0)], 0.8, DARK); k.path(&[(8.0, 6.8), (8.0, 9.2)], 0.8, DARK); }
        SkCircle3 => { k.ring((8.0, 8.0), 5.8, 1.5, SK); for t in [100.0f32, 220.0, 340.0] { let r = t.to_radians(); k.sq((8.0 + 5.8 * r.cos(), 8.0 - 5.8 * r.sin()), 1.1, DARK); } }
        SkArc3 => { let a = k.arc((8.0, 10.0), 6.0, 6.0, 10.0, 170.0); k.path(&a, 1.6, SK); k.sq(a[0], 1.1, DARK); k.sq(a[a.len() - 1], 1.1, DARK); k.sq(a[a.len() / 2], 1.1, DARK); }
        SkArcCenter => {
            let a = k.arc((5.0, 11.0), 8.0, 8.0, 0.0, 90.0);
            k.path(&a, 1.6, SK);
            k.dashed((5.0, 11.0), a[0], 0.7, BLUE_L, 1.5, 1.0);
            k.dashed((5.0, 11.0), a[a.len() - 1], 0.7, BLUE_L, 1.5, 1.0);
            k.sq((5.0, 11.0), 1.1, DARK);
        }
        SkFillet => { k.path(&[(3.0, 15.0), (3.0, 8.0)], 1.5, SK); k.path(&[(8.0, 3.0), (15.0, 3.0)], 1.5, SK); let a = k.arc((8.0, 8.0), 5.0, 5.0, 90.0, 180.0); k.path(&a, 1.6, SK); }
        SkChamfer => k.path(&[(3.0, 15.0), (3.0, 8.0), (8.0, 3.0), (15.0, 3.0)], 1.5, SK),
        SkCenterline => {
            k.dashed((2.0, 14.0), (14.0, 2.0), 1.1, DARK, 3.4, 1.4);
            k.sq((8.0, 8.0), 0.6, DARK);
        }
        DispReflect => {
            k.iso_pal((8.0, 15.0), 5.2, 5.2, 5.2, [rgb(214, 232, 250), rgb(150, 182, 220), rgb(96, 130, 176), rgb(70, 96, 136)]);
            k.path(&[(9.5, 5.5), (12.0, 4.2)], 1.2, WHITE);
        }
        Reorient => {
            k.iso_pal((7.0, 14.5), 4.6, 4.6, 4.6, [GTOP, GLEFT, GRIGHT, GLINE]);
            let a = k.arc((8.0, 8.0), 6.5, 6.5, 20.0, 160.0);
            k.path(&a, 1.2, BLUE);
            k.head(a[0], a[2], 2.6, BLUE);
        }
        NormalView => {
            k.poly(&[(2.0, 4.0), (10.0, 4.0), (10.0, 14.0), (2.0, 14.0)], rgb(220, 234, 250), Some(BLUE));
            k.arrow((15.0, 9.0), (10.6, 9.0), 1.4, GREEN_D, 3.0);
        }
        SkCheck => {
            k.closed(&[(2.0, 4.0), (11.0, 4.0), (11.0, 12.0), (2.0, 12.0)], 1.3, SK);
            k.circle((11.5, 11.5), 3.6, rgb(220, 238, 252), Some((1.1, DARK)));
            k.path(&[(10.0, 11.6), (11.2, 12.8), (13.2, 10.2)], 1.2, GREEN_D);
        }
        SkLock => {
            k.poly(&[(4.0, 8.0), (12.0, 8.0), (12.0, 14.5), (4.0, 14.5)], rgb(110, 116, 124), Some(DARK));
            let a = k.arc((8.0, 8.0), 3.0, 4.0, 0.0, 180.0);
            k.path(&a, 1.5, DARK);
        }
        HlOverlap => {
            k.closed(&[(2.0, 3.0), (10.0, 3.0), (10.0, 11.0), (2.0, 11.0)], 1.2, SK);
            k.path(&[(10.0, 5.0), (10.0, 14.0), (14.0, 14.0)], 1.2, SK);
            k.path(&[(10.0, 5.0), (10.0, 11.0)], 2.4, rgb(255, 120, 0));
        }
        HlIntersect => {
            k.ring((6.5, 9.0), 4.5, 1.2, SK);
            k.path(&[(1.5, 3.0), (14.5, 13.0)], 1.2, SK);
            k.ring((10.4, 10.0), 1.8, 1.2, RED);
        }
        HlBranch => {
            k.path(&[(2.0, 3.0), (8.0, 9.0), (14.0, 3.0)], 1.3, SK);
            k.path(&[(8.0, 9.0), (8.0, 15.0)], 1.3, SK);
            k.closed(&[(6.0, 7.0), (10.0, 7.0), (10.0, 11.0), (6.0, 11.0)], 1.1, rgb(200, 0, 200));
        }
        SkLineTangent | SkCenterlineTangent => {
            k.ring((4.5, 9.5), 3.0, 1.2, SK);
            k.ring((11.5, 6.5), 3.0, 1.2, SK);
            if icon == SkLineTangent { k.path(&[(2.3, 6.8), (10.0, 3.6)], 1.6, SK); } else { k.dashed((1.5, 7.2), (12.0, 2.8), 1.1, DARK, 2.4, 1.2); }
        }
        SkSlantRect => { let q = [(2.0, 9.0), (8.0, 3.0), (14.0, 9.0), (8.0, 15.0)]; k.closed(&q, 1.5, SK); for x in q { k.sq(x, 1.1, DARK); } }
        SkParallelogram => { let q = [(1.5, 12.0), (5.0, 4.0), (14.5, 4.0), (11.0, 12.0)]; k.closed(&q, 1.5, SK); for x in q { k.sq(x, 1.1, DARK); } }
        SkCircleConc => { k.ring((8.0, 8.0), 6.2, 1.4, SK); k.ring((8.0, 8.0), 3.4, 1.4, SK); k.sq((8.0, 8.0), 0.9, DARK); }
        SkCircle3T => {
            k.ring((8.0, 8.5), 4.0, 1.4, SK);
            k.path(&[(1.5, 13.0), (14.5, 13.0)], 1.0, DARK);
            k.path(&[(1.0, 9.0), (6.0, 1.0)], 1.0, DARK);
            k.path(&[(15.0, 9.0), (10.0, 1.0)], 1.0, DARK);
        }
        SkArc3T => {
            let a = k.arc((8.0, 8.5), 4.0, 4.0, 200.0, 340.0);
            k.path(&a, 1.5, SK);
            k.path(&[(1.0, 9.0), (6.0, 1.0)], 1.0, DARK);
            k.path(&[(15.0, 9.0), (10.0, 1.0)], 1.0, DARK);
            k.path(&[(1.5, 13.0), (14.5, 13.0)], 1.0, DARK);
        }
        SkArcConc => {
            let a = k.arc((8.0, 11.0), 6.5, 6.5, 10.0, 170.0);
            k.path(&a, 1.5, SK);
            let b = k.arc((8.0, 11.0), 3.5, 3.5, 10.0, 170.0);
            k.path(&b, 1.5, SK);
            k.sq((8.0, 11.0), 0.9, DARK);
        }
        SkConic => {
            let c: Vec<P> = (0..=16).map(|i| { let t = i as f32 / 16.0; let (x0, y0, cx, cy, x1, y1) = (2.0, 13.0, 3.0, 2.0, 14.0, 3.0); ((1.0 - t) * (1.0 - t) * x0 + 2.0 * t * (1.0 - t) * cx + t * t * x1, (1.0 - t) * (1.0 - t) * y0 + 2.0 * t * (1.0 - t) * cy + t * t * y1) }).collect();
            k.path(&c, 1.5, SK);
            k.dashed((2.0, 13.0), (3.0, 2.0), 0.7, MID, 1.4, 1.0);
            k.dashed((3.0, 2.0), (14.0, 3.0), 0.7, MID, 1.4, 1.0);
            k.sq(c[0], 1.0, DARK); k.sq(c[16], 1.0, DARK);
        }
        SkEllipseAxis => {
            let e = k.arc((8.0, 8.0), 6.6, 4.0, 0.0, 360.0);
            k.closed(&e, 1.5, SK);
            k.sq((1.4, 8.0), 1.1, DARK); k.sq((14.6, 8.0), 1.1, DARK);
        }
        SkFilletTrim => { k.path(&[(3.0, 15.0), (3.0, 8.0)], 1.5, SK); k.path(&[(8.0, 3.0), (15.0, 3.0)], 1.5, SK); let a = k.arc((8.0, 8.0), 5.0, 5.0, 90.0, 180.0); k.path(&a, 1.6, SK); k.dashed((3.0, 8.0), (3.0, 3.0), 0.8, BLUE_L, 1.2, 1.0); k.dashed((3.0, 3.0), (8.0, 3.0), 0.8, BLUE_L, 1.2, 1.0); }
        SkFilletEllip => { k.path(&[(3.0, 15.0), (3.0, 9.0)], 1.5, SK); k.path(&[(6.0, 3.0), (15.0, 3.0)], 1.5, SK); let a = k.arc((6.0, 9.0), 3.0, 6.0, 90.0, 180.0); k.path(&a, 1.6, SK); }
        SkChamferTrim => { k.path(&[(3.0, 15.0), (3.0, 8.0), (8.0, 3.0), (15.0, 3.0)], 1.5, SK); k.dashed((3.0, 8.0), (3.0, 3.0), 0.8, BLUE_L, 1.2, 1.0); k.dashed((3.0, 3.0), (8.0, 3.0), 0.8, BLUE_L, 1.2, 1.0); }
        SelChain => {
            k.path(&[(2.0, 13.0), (2.0, 6.0), (9.0, 6.0)], 1.4, SK);
            k.poly(&[(9.0, 6.0), (9.0, 15.0), (11.0, 13.0), (12.6, 15.8), (13.8, 15.0), (12.2, 12.4), (14.6, 12.2)], WHITE, Some(DARK));
        }
        SelAll => {
            k.closed(&[(1.5, 1.5), (12.0, 1.5), (12.0, 10.0), (1.5, 10.0)], 1.0, SK);
            k.poly(&[(8.0, 6.0), (8.0, 15.0), (10.0, 13.0), (11.6, 15.8), (12.8, 15.0), (11.2, 12.4), (13.6, 12.2)], WHITE, Some(DARK));
        }
        SkReplace => {
            k.path(&[(2.0, 5.0), (9.0, 5.0)], 1.5, SK);
            k.path(&[(7.0, 12.0), (14.0, 12.0)], 1.5, GREEN_D);
            let a = k.arc((8.0, 8.5), 4.0, 3.5, 300.0, 420.0);
            k.path(&a, 1.0, DARK);
        }
        SkConvert => { k.path(&[(2.0, 12.0), (14.0, 12.0)], 1.5, SK); k.arrow((8.0, 9.0), (8.0, 2.0), 1.3, GREEN_D, 3.0); }
        InfoIntersect | InfoTangent | InfoEntity => {
            match icon {
                InfoIntersect => { k.path(&[(1.5, 13.5), (11.0, 4.0)], 1.3, SK); k.path(&[(1.5, 4.0), (11.0, 13.5)], 1.3, SK); }
                InfoTangent => { k.ring((6.0, 9.5), 4.0, 1.3, SK); k.path(&[(1.5, 13.5), (11.0, 13.5)], 1.3, SK); }
                _ => { k.ring((6.5, 9.0), 4.8, 1.3, SK); }
            }
            k.circle((12.5, 4.0), 3.2, BLUE, None);
            k.text((12.5, 4.1), "i", 5.0, WHITE);
        }
        SketchSetup => {
            k.plane([(1.5, 11.0), (8.0, 7.5), (14.5, 11.0), (8.0, 14.5)]);
            k.pencil((6.0, 9.5));
        }
        References => {
            k.dashed((1.5, 8.0), (14.5, 8.0), 1.0, rgb(214, 44, 214), 2.2, 1.2);
            k.dashed((8.0, 1.5), (8.0, 14.5), 1.0, rgb(214, 44, 214), 2.2, 1.2);
            k.ring((8.0, 8.0), 2.4, 1.2, DARK);
        }
        Cut => {
            k.ring((4.5, 12.0), 2.3, 1.3, DARK);
            k.ring((11.5, 12.0), 2.3, 1.3, DARK);
            k.path(&[(6.0, 10.4), (12.0, 1.5)], 1.3, MID);
            k.path(&[(10.0, 10.4), (4.0, 1.5)], 1.3, MID);
        }
        SkText => { k.text((8.0, 8.5), "A", 13.0, SK); k.path(&[(2.0, 15.0), (14.0, 15.0)], 0.8, DARK); }
        SkCsys => {
            k.arrow((4.0, 12.0), (14.0, 12.0), 1.2, DARK, 2.6);
            k.arrow((4.0, 12.0), (4.0, 2.0), 1.2, DARK, 2.6);
            k.ring((4.0, 12.0), 1.4, 1.0, SK);
        }
        SkOffset => {
            k.path(&[(2.0, 13.0), (8.0, 13.0), (13.0, 8.0)], 1.0, MID);
            k.path(&[(2.0, 9.0), (6.5, 9.0), (10.0, 5.5)], 1.5, SK);
            k.arrow((11.0, 11.0), (8.6, 8.6), 0.9, GREEN_D, 2.0);
        }
        SkThicken => {
            k.path(&[(2.0, 13.0), (13.0, 2.0)], 1.0, MID);
            k.path(&[(2.0, 9.0), (9.0, 2.0)], 1.5, SK);
            k.path(&[(6.0, 15.0), (15.0, 6.0)], 1.5, SK);
            k.path(&[(2.0, 9.0), (6.0, 15.0)], 1.2, SK);
        }
        RotResize => {
            k.closed(&[(2.0, 6.0), (9.0, 6.0), (9.0, 13.0), (2.0, 13.0)], 1.2, SK);
            let a = k.arc((9.0, 6.0), 5.0, 5.0, 0.0, 120.0);
            k.path(&a, 1.1, GREEN_D);
            k.head(a[0], a[3], 2.4, GREEN_D);
        }
        RefDim => { k.text((8.0, 8.5), "REF", 6.0, DARK); k.path(&[(1.5, 3.0), (1.5, 13.0)], 0.8, DARK); k.path(&[(14.5, 3.0), (14.5, 13.0)], 0.8, DARK); }
        Perimeter => {
            k.closed(&[(3.0, 6.0), (13.0, 6.0), (13.0, 14.0), (3.0, 14.0)], 1.3, SK);
            k.arrow((8.0, 3.0), (14.5, 3.0), 0.9, DARK, 2.0);
            k.arrow((8.0, 3.0), (1.5, 3.0), 0.9, DARK, 2.0);
        }
        Baseline => {
            k.path(&[(2.0, 14.0), (2.0, 2.0)], 1.5, SK);
            for y in [4.0, 9.0, 13.5] { k.path(&[(2.0, y), (12.0, y)], 0.7, MID); }
            k.text((9.5, 7.0), "0", 6.0, DARK);
        }
        SkImport => {
            k.poly(&[(3.0, 1.5), (10.5, 1.5), (13.0, 4.0), (13.0, 14.5), (3.0, 14.5)], WHITE, Some(MID));
            k.arrow((8.0, 4.0), (8.0, 12.5), 1.4, GREEN_D, 3.0);
        }
        SkPoint => { k.path(&[(5.0, 5.0), (11.0, 11.0)], 1.5, SK); k.path(&[(11.0, 5.0), (5.0, 11.0)], 1.5, SK); }
        SkPalette => {
            let pts: Vec<P> = (0..6).map(|i| { let t = (i as f32 * 60.0 + 30.0).to_radians(); (8.0 + 6.2 * t.cos(), 8.0 - 6.2 * t.sin()) }).collect();
            k.closed(&pts, 1.5, SK);
        }
        SkSpline => {
            let c: Vec<P> = (0..=24).map(|i| { let t = i as f32 / 24.0; (2.0 + 12.0 * t, 9.0 - 5.0 * (t * 6.28).sin()) }).collect();
            k.path(&c, 1.5, SK);
            for q in [c[0], c[6], c[18], c[24]] { k.sq(q, 1.1, DARK); }
        }
        SkEllipse => {
            let e = k.arc((8.0, 8.0), 6.6, 4.0, 0.0, 360.0);
            k.closed(&e, 1.5, SK);
            k.path(&[(7.0, 8.0), (9.0, 8.0)], 0.8, DARK);
            k.path(&[(8.0, 7.0), (8.0, 9.0)], 0.8, DARK);
        }
        SkDeleteSeg => {
            k.path(&[(1.5, 11.0), (14.5, 11.0)], 1.5, SK);
            k.path(&[(5.5, 3.5), (10.5, 8.5)], 1.6, RED);
            k.path(&[(10.5, 3.5), (5.5, 8.5)], 1.6, RED);
        }
        SkConstruction => {
            k.dashed((2.5, 4.0), (13.5, 4.0), 1.2, BLUE_L, 2.0, 1.2);
            k.dashed((13.5, 4.0), (13.5, 12.0), 1.2, BLUE_L, 2.0, 1.2);
            k.dashed((13.5, 12.0), (2.5, 12.0), 1.2, BLUE_L, 2.0, 1.2);
            k.dashed((2.5, 12.0), (2.5, 4.0), 1.2, BLUE_L, 2.0, 1.2);
        }
        SkDimension | DimDisplay | EditDims => {
            k.path(&[(2.0, 5.0), (2.0, 14.0)], 0.8, DARK);
            k.path(&[(14.0, 5.0), (14.0, 14.0)], 0.8, DARK);
            k.arrow((8.0, 11.0), (2.2, 11.0), 1.0, BLUE, 2.6);
            k.arrow((8.0, 11.0), (13.8, 11.0), 1.0, BLUE, 2.6);
            k.text((8.0, 6.0), if icon == EditDims { "d" } else { "10" }, 5.5, rgb(28, 52, 150));
        }
        SkModify => {
            k.path(&[(1.5, 4.0), (1.5, 12.0)], 0.8, DARK);
            k.path(&[(10.0, 4.0), (10.0, 12.0)], 0.8, DARK);
            k.arrow((6.0, 9.5), (1.7, 9.5), 0.9, BLUE, 2.2);
            k.arrow((6.0, 9.5), (9.8, 9.5), 0.9, BLUE, 2.2);
            k.pencil((9.0, 15.0));
        }
        SkMirror => {
            k.dashed((8.0, 1.0), (8.0, 15.0), 1.0, DARK, 3.0, 1.2);
            k.closed(&[(2.0, 4.0), (6.0, 7.0), (2.0, 12.0)], 1.4, SK);
            k.closed(&[(14.0, 4.0), (10.0, 7.0), (14.0, 12.0)], 1.0, BLUE_L);
        }
        SkDivide => { k.path(&[(1.5, 10.0), (7.0, 10.0)], 1.5, SK); k.path(&[(9.0, 10.0), (14.5, 10.0)], 1.5, SK); k.sq((8.0, 10.0), 1.4, RED); k.path(&[(8.0, 3.0), (8.0, 7.0)], 1.0, RED); }
        SkCorner => {
            k.path(&[(2.0, 13.0), (9.0, 13.0)], 1.5, SK);
            k.path(&[(13.0, 2.0), (13.0, 9.0)], 1.5, SK);
            k.dashed((9.0, 13.0), (13.0, 13.0), 1.0, RED, 1.2, 1.0);
            k.dashed((13.0, 9.0), (13.0, 13.0), 1.0, RED, 1.2, 1.0);
            k.sq((13.0, 13.0), 1.2, DARK);
        }
        SkProject => {
            k.iso((8.0, 15.5), 5.5, 5.5, 3.0);
            k.path(&[(3.24, 10.25), (8.0, 7.5), (12.76, 10.25)], 1.8, SK);
            k.arrow((8.0, 0.5), (8.0, 5.8), 1.2, GREEN, 2.6);
        }
        ConV => { k.path(&[(8.0, 2.0), (8.0, 14.0)], 1.6, SK); k.text((12.5, 8.0), "V", 6.0, BLUE); }
        ConH => { k.path(&[(2.0, 10.0), (14.0, 10.0)], 1.6, SK); k.text((8.0, 5.0), "H", 6.0, BLUE); }
        ConPerp => { k.path(&[(8.0, 2.0), (8.0, 13.5)], 1.5, SK); k.path(&[(2.0, 13.5), (14.0, 13.5)], 1.5, SK); k.closed(&[(8.0, 10.5), (11.0, 10.5), (11.0, 13.5)], 0.8, BLUE); }
        ConTan => { k.ring((7.0, 9.5), 4.5, 1.4, SK); k.path(&[(1.0, 5.0), (15.0, 5.0)], 1.4, SK); }
        ConMid => { k.path(&[(2.0, 13.0), (14.0, 3.0)], 1.4, SK); k.circle((8.0, 8.0), 2.0, BLUE, None); }
        ConCoinc => { k.path(&[(2.0, 13.0), (8.0, 8.0)], 1.4, SK); k.path(&[(8.0, 8.0), (14.0, 11.0)], 1.4, SK); k.circle((8.0, 8.0), 1.9, BLUE, None); }
        ConSym => {
            k.dashed((8.0, 1.0), (8.0, 15.0), 0.9, DARK, 2.6, 1.0);
            k.circle((3.5, 8.0), 1.5, SK, None);
            k.circle((12.5, 8.0), 1.5, SK, None);
            k.arrow((6.0, 12.0), (2.5, 12.0), 0.8, BLUE, 2.0);
            k.arrow((10.0, 12.0), (13.5, 12.0), 0.8, BLUE, 2.0);
        }
        ConEqual => { k.path(&[(3.0, 6.0), (13.0, 6.0)], 1.6, SK); k.path(&[(3.0, 10.5), (13.0, 10.5)], 1.6, SK); k.text((8.0, 13.8), "=", 5.0, BLUE); }
        ConPar => { k.path(&[(2.0, 12.0), (9.0, 3.0)], 1.6, SK); k.path(&[(7.0, 13.0), (14.0, 4.0)], 1.6, SK); }
        SketchView => {
            k.poly(&[(3.0, 3.0), (13.0, 3.0), (13.0, 13.0), (3.0, 13.0)], TAN, Some(BROWN));
            k.path(&[(5.0, 10.5), (8.0, 6.0), (11.0, 9.0)], 1.3, SK);
            k.circle((12.5, 12.5), 2.6, WHITE, Some((1.0, DARK)));
            k.circle((12.5, 12.5), 1.0, BLUE, None);
        }
        Requirements => {
            k.poly(&[(3.0, 2.5), (13.0, 2.5), (13.0, 14.5), (3.0, 14.5)], WHITE, Some(MID));
            k.poly(&[(6.0, 1.5), (10.0, 1.5), (10.0, 3.5), (6.0, 3.5)], MID, None);
            k.path(&[(5.0, 9.0), (7.5, 11.8), (11.5, 6.0)], 1.8, GREEN);
        }
        OpenEnds => { k.path(&[(3.0, 12.0), (13.0, 4.0)], 1.5, SK); k.closed(&[(1.5, 10.5), (4.5, 10.5), (4.5, 13.5), (1.5, 13.5)], 1.1, RED); k.closed(&[(11.5, 2.5), (14.5, 2.5), (14.5, 5.5), (11.5, 5.5)], 1.1, RED); }
        ShadeLoops => {
            let pts = [(2.0, 13.0), (5.0, 3.0), (11.0, 2.5), (14.5, 9.0), (10.0, 14.0)];
            k.poly(&pts, rgb(255, 205, 140), None);
            k.closed(&pts, 1.3, SK);
        }
        Grid => {
            for i in 0..5 { let t = 2.0 + i as f32 * 3.0; k.path(&[(t, 2.0), (t, 14.0)], 0.7, MID); k.path(&[(2.0, t), (14.0, t)], 0.7, MID); }
        }
        Vertex => { k.path(&[(2.0, 12.0), (8.0, 4.0), (14.0, 12.0)], 1.2, SK); for q in [(2.0, 12.0), (8.0, 4.0), (14.0, 12.0)] { k.sq(q, 1.4, DARK); } }
        ConDisplay => { k.text((5.5, 8.5), "H", 8.0, BLUE); k.text((11.5, 8.5), "V", 8.0, BLUE); }
        Ok => {
            k.path(&[(2.6, 8.6), (6.4, 12.6), (13.8, 3.8)], 3.0, GREEN_D);
            k.path(&[(2.6, 8.6), (6.4, 12.6), (13.8, 3.8)], 2.0, GREEN);
        }
        Cancel => {
            k.path(&[(3.5, 3.5), (12.5, 12.5)], 2.6, RED);
            k.path(&[(12.5, 3.5), (3.5, 12.5)], 2.6, RED);
        }
        Preview => {
            k.ring((4.6, 9.0), 3.0, 1.3, DARK);
            k.ring((11.4, 9.0), 3.0, 1.3, DARK);
            k.path(&[(7.6, 8.4), (8.4, 8.4)], 1.2, DARK);
            k.path(&[(1.6, 8.4), (0.6, 5.6)], 1.0, DARK);
            k.path(&[(14.4, 8.4), (15.4, 5.6)], 1.0, DARK);
            k.circle((4.6, 9.0), 2.2, rgb(200, 225, 250), None);
            k.circle((11.4, 9.0), 2.2, rgb(200, 225, 250), None);
        }
        Flip => {
            k.arrow((5.5, 13.5), (5.5, 2.0), 1.6, BLUE, 3.4);
            k.arrow((10.5, 2.5), (10.5, 14.0), 1.6, BLUE, 3.4);
        }
        RemoveMaterial => {
            k.iso((8.0, 15.0), 5.0, 5.0, 5.0);
            let c = Ic::iso_top_center((8.0, 15.0), 5.0, 5.0, 5.0);
            let s2 = 0.45;
            let pts: Vec<P> = [(0.0, 2.5), (4.33, 0.0), (0.0, -2.5), (-4.33, 0.0)].iter().map(|q| (c.0 + q.0 * s2, c.1 + q.1 * s2)).collect();
            k.poly(&pts, WHITE, Some(YO));
            k.arrow((c.0, c.1 - 6.0), (c.0, c.1 + 0.8), 1.2, RED, 2.4);
        }
        Thicken => {
            let c: Vec<P> = (0..=16).map(|i| { let t = i as f32 / 16.0; (2.0 + 12.0 * t, 10.0 - 4.0 * (t * 3.14).sin()) }).collect();
            k.path(&c, 3.6, YL);
            let up: Vec<P> = c.iter().map(|q| (q.0, q.1 - 1.6)).collect();
            let lo: Vec<P> = c.iter().map(|q| (q.0, q.1 + 1.6)).collect();
            k.path(&up, 0.8, YO);
            k.path(&lo, 0.8, YO);
        }
        DepthBlind => { k.path(&[(2.0, 3.0), (2.0, 13.0)], 1.6, BROWN); k.arrow((2.0, 8.0), (12.5, 8.0), 1.4, BLUE, 3.0); k.path(&[(13.5, 4.0), (13.5, 12.0)], 1.2, DARK); }
        DepthSymmetric => {
            k.path(&[(8.0, 3.0), (8.0, 13.0)], 1.6, BROWN);
            k.arrow((8.0, 8.0), (14.0, 8.0), 1.2, BLUE, 2.6);
            k.arrow((8.0, 8.0), (2.0, 8.0), 1.2, BLUE, 2.6);
            k.path(&[(15.0, 4.5), (15.0, 11.5)], 1.0, DARK);
            k.path(&[(1.0, 4.5), (1.0, 11.5)], 1.0, DARK);
        }
        DepthThrough => {
            for x in [4.0, 8.0, 12.0] { k.path(&[(x, 3.0), (x, 13.0)], 1.2, BROWN); }
            k.arrow((1.0, 8.0), (15.5, 8.0), 1.4, BLUE, 3.0);
        }
        DepthNext => {
            k.path(&[(2.0, 3.0), (2.0, 13.0)], 1.6, BROWN);
            k.arrow((2.0, 8.0), (10.5, 8.0), 1.4, BLUE, 3.0);
            k.iso((13.0, 15.0), 2.0, 2.0, 9.0);
        }
        DepthSelected => {
            k.path(&[(2.0, 3.0), (2.0, 13.0)], 1.6, BROWN);
            k.arrow((2.0, 8.0), (11.0, 8.0), 1.4, BLUE, 3.0);
            k.poly(&[(11.5, 15.0), (14.0, 11.0), (14.0, 1.0), (11.5, 5.0)], rgb(150, 220, 150), Some(GREEN_D));
        }
        Taper => {
            k.poly(&[(4.0, 14.0), (12.0, 14.0), (10.0, 3.0), (6.0, 3.0)], YL, Some(YO));
            k.dashed((12.0, 14.0), (12.0, 2.0), 0.8, DARK, 1.4, 1.0);
            let a = k.arc((12.0, 14.0), 7.0, 7.0, 90.0, 100.0);
            k.path(&a, 1.0, BLUE);
        }
        HoleSimple => k.hole_section(0),
        HoleCbore => k.hole_section(1),
        HoleCsink => k.hole_section(2),
        HoleStandard => {
            k.hole_section(0);
            for i in 0..5 {
                let y = 5.0 + i as f32 * 2.0;
                k.path(&[(5.5, y), (4.4, y + 1.0)], 0.9, rgb(220, 120, 30));
                k.path(&[(10.5, y), (11.6, y + 1.0)], 0.9, rgb(220, 120, 30));
            }
        }
        Delete => {
            k.path(&[(3.5, 3.5), (12.5, 12.5)], 2.2, RED);
            k.path(&[(12.5, 3.5), (3.5, 12.5)], 2.2, RED);
        }
        Suppress => {
            k.iso_pal((8.0, 15.0), 4.6, 4.6, 5.0, [GTOP, GLEFT, GRIGHT, GLINE]);
            k.circle((11.5, 4.5), 3.6, WHITE, Some((1.2, RED)));
            k.path(&[(9.0, 7.0), (14.0, 2.0)], 1.2, RED);
        }
        Resume => {
            k.iso((7.0, 15.0), 4.2, 4.2, 4.6);
            k.arrow((10.0, 6.0), (15.0, 1.5), 1.4, GREEN, 3.0);
        }
        Hide | Show => {
            k.eye();
            if icon == Hide { k.path(&[(2.0, 14.0), (14.0, 2.0)], 1.6, RED); }
        }
        EditDef => { k.iso((7.0, 15.0), 4.4, 4.4, 4.6); k.pencil((8.5, 10.0)); }
        Part => k.iso((8.0, 15.0), 5.0, 5.0, 5.0),
        InsertHere => {
            k.path(&[(2.0, 8.0), (11.0, 8.0)], 2.4, GREEN);
            k.head((15.0, 8.0), (2.0, 8.0), 5.6, GREEN);
        }
        Failed => { k.circle((8.0, 8.0), 6.5, RED, None); k.path(&[(5.0, 5.0), (11.0, 11.0)], 1.8, WHITE); k.path(&[(11.0, 5.0), (5.0, 11.0)], 1.8, WHITE); }
        TreeTab => {
            k.path(&[(3.0, 3.0), (3.0, 13.0)], 1.0, DARK);
            for y in [6.0, 10.0, 13.0] { k.path(&[(3.0, y), (7.0, y)], 1.0, DARK); }
            k.iso((4.0, 4.5), 2.0, 2.0, 2.6);
            for y in [6.0, 10.0, 13.0] { k.poly(&[(8.0, y - 1.3), (14.0, y - 1.3), (14.0, y + 1.3), (8.0, y + 1.3)], if y == 6.0 { YL } else { LIGHT }, Some(MID)); }
        }
        FileItem => {
            k.poly(&[(3.5, 1.5), (10.0, 1.5), (12.5, 4.0), (12.5, 14.5), (3.5, 14.5)], WHITE, Some(MID));
            k.iso((8.0, 13.0), 2.6, 2.6, 3.0);
        }
        Up => { k.arrow((8.0, 14.0), (8.0, 2.0), 1.8, BLUE, 4.0); }
        RegenOk => { k.p.rect_filled(Rect::from_min_max(k.pt((3.0, 3.0)), k.pt((13.0, 13.0))), 2.0, k.c(rgb(60, 180, 70))); }
        RegenFail => { k.p.rect_filled(Rect::from_min_max(k.pt((3.0, 3.0)), k.pt((13.0, 13.0))), 2.0, k.c(rgb(220, 60, 50))); }
        Assembly | ReturnAsm => {
            k.iso((8.0, 15.5), 6.0, 6.0, 3.2);
            k.iso_pal((8.0, 9.6), 3.2, 3.2, 3.6, [rgb(220, 228, 238), GLEFT, GRIGHT, GLINE]);
            if icon == ReturnAsm { k.arrow((13.0, 8.0), (13.0, 1.0), 1.5, GREEN, 3.0); }
        }
        Assemble => {
            k.iso((7.5, 15.5), 6.0, 5.0, 3.4);
            k.iso_pal((7.5, 7.0), 3.0, 3.0, 3.4, [rgb(220, 228, 238), GLEFT, GRIGHT, GLINE]);
            k.arrow((13.0, 1.5), (13.0, 9.0), 1.5, GREEN, 3.0);
        }
        CreateComp => {
            k.iso((7.0, 15.5), 5.0, 5.0, 5.0);
            k.poly(&[(12.5, 1.0), (13.4, 3.6), (12.5, 6.2), (11.6, 3.6)], rgb(250, 190, 40), None);
            k.poly(&[(9.9, 3.6), (12.5, 2.7), (15.1, 3.6), (12.5, 4.5)], rgb(250, 190, 40), None);
        }
        Explode => {
            k.iso((8.0, 16.0), 4.4, 4.4, 2.6);
            k.iso_pal((8.0, 9.8), 3.0, 3.0, 2.6, [rgb(220, 228, 238), GLEFT, GRIGHT, GLINE]);
            k.iso_pal((8.0, 4.2), 2.0, 2.0, 2.2, [rgb(200, 226, 250), BLUE_L, rgb(110, 150, 210), BLUE]);
            k.arrow((14.0, 9.0), (14.0, 2.0), 1.2, GREEN, 2.6);
        }
        Bom => {
            k.poly(&[(2.5, 1.5), (13.5, 1.5), (13.5, 14.5), (2.5, 14.5)], WHITE, Some(MID));
            k.poly(&[(2.5, 1.5), (13.5, 1.5), (13.5, 4.0), (2.5, 4.0)], BLUE_L, Some(MID));
            for y in [6.5, 9.0, 11.5] { k.path(&[(4.0, y), (5.5, y)], 1.2, BLUE); k.path(&[(7.0, y), (12.0, y)], 1.0, DARK); }
        }
        Interference => {
            k.iso((6.5, 15.0), 5.0, 4.0, 5.0);
            k.iso_pal((11.0, 12.0), 4.0, 4.0, 5.0, [rgb(220, 228, 238), GLEFT, GRIGHT, GLINE]);
            k.circle((8.5, 8.0), 2.4, RED, None);
        }
        DragComp => {
            k.iso((7.0, 15.5), 5.0, 5.0, 4.0);
            k.arrow((11.0, 5.0), (15.0, 5.0), 1.2, BLUE, 2.4);
            k.arrow((11.0, 5.0), (11.0, 1.0), 1.2, BLUE, 2.4);
        }
        AsmAuto => {
            k.plane([(1.5, 13.0), (9.0, 13.0), (11.5, 9.0), (4.0, 9.0)]);
            k.poly(&[(10.5, 1.0), (6.5, 7.5), (9.2, 7.5), (7.5, 12.0), (13.0, 5.0), (10.0, 5.0), (12.0, 1.0)], rgb(250, 196, 40), Some(rgb(170, 120, 20)));
        }
        AsmDistance => {
            k.plane([(1.5, 14.5), (9.5, 14.5), (12.0, 11.0), (4.0, 11.0)]);
            k.plane([(1.5, 6.0), (9.5, 6.0), (12.0, 2.5), (4.0, 2.5)]);
            k.arrow((13.5, 8.5), (13.5, 3.0), 1.1, BLUE, 2.4);
            k.arrow((13.5, 8.5), (13.5, 14.0), 1.1, BLUE, 2.4);
        }
        AsmAngle => {
            k.path(&[(2.0, 14.0), (14.5, 14.0)], 2.0, BROWN);
            k.path(&[(2.0, 14.0), (11.5, 3.0)], 2.0, BROWN);
            let a = k.arc((2.0, 14.0), 8.0, 8.0, 0.0, 49.0);
            k.path(&a, 1.1, BLUE);
        }
        AsmParallel => {
            k.path(&[(2.0, 13.0), (9.0, 3.0)], 2.0, BROWN);
            k.path(&[(7.0, 13.0), (14.0, 3.0)], 2.0, BROWN);
            k.path(&[(1.5, 15.0), (14.5, 15.0)], 0.8, MID);
        }
        AsmCoincident => {
            k.plane([(1.5, 13.0), (9.5, 13.0), (13.0, 8.0), (5.0, 8.0)]);
            k.poly(&[(2.5, 10.5), (10.5, 10.5), (14.0, 5.5), (6.0, 5.5)], rgb(200, 226, 250), Some(BLUE));
            k.circle((8.2, 8.0), 1.6, GREEN, None);
        }
        AsmNormal => {
            k.path(&[(2.0, 14.0), (14.0, 14.0)], 2.0, BROWN);
            k.path(&[(8.0, 14.0), (8.0, 2.0)], 2.0, BROWN);
            k.closed(&[(8.0, 11.0), (11.0, 11.0), (11.0, 14.0)], 0.9, BLUE);
        }
        AsmDefault => {
            for (d, c) in [((14.0, 11.0), RED), ((3.0, 13.5), GREEN), ((8.0, 1.5), BLUE)] { k.arrow((8.0, 9.0), d, 1.2, c, 2.6); }
            k.text((12.0, 3.5), "D", 6.0, DARK);
        }
        Drawing | Sheet | NewSheet | TitleBlock | Pdf | Dxf | Svg | ReturnDwg => {
            k.poly(&[(2.5, 1.5), (13.5, 1.5), (13.5, 14.5), (2.5, 14.5)], WHITE, Some(MID));
            match icon {
                Drawing | Sheet | ReturnDwg => {
                    k.closed(&[(4.0, 3.5), (9.0, 3.5), (9.0, 8.0), (4.0, 8.0)], 0.9, DARK);
                    k.closed(&[(10.0, 3.5), (12.0, 3.5), (12.0, 8.0), (10.0, 8.0)], 0.9, DARK);
                    k.poly(&[(8.0, 11.0), (13.5, 11.0), (13.5, 14.5), (8.0, 14.5)], BLUE_L, Some(MID));
                    if icon == ReturnDwg { k.arrow((5.0, 13.0), (5.0, 9.5), 1.3, GREEN, 2.6); }
                }
                NewSheet => {
                    k.poly(&[(8.0, 11.0), (13.5, 11.0), (13.5, 14.5), (8.0, 14.5)], BLUE_L, Some(MID));
                    k.path(&[(6.0, 4.0), (6.0, 10.0)], 1.8, GREEN);
                    k.path(&[(3.0, 7.0), (9.0, 7.0)], 1.8, GREEN);
                }
                TitleBlock => {
                    k.poly(&[(6.0, 9.5), (13.5, 9.5), (13.5, 14.5), (6.0, 14.5)], BLUE_L, Some(DARK));
                    k.path(&[(6.0, 12.0), (13.5, 12.0)], 0.7, DARK);
                    k.path(&[(9.5, 9.5), (9.5, 14.5)], 0.7, DARK);
                }
                _ => {
                    let (t, c) = match icon { Pdf => ("PDF", RED), Dxf => ("DXF", BLUE), _ => ("SVG", GREEN_D) };
                    k.poly(&[(1.0, 8.0), (15.0, 8.0), (15.0, 13.0), (1.0, 13.0)], c, None);
                    k.text((8.0, 10.6), t, 4.6, WHITE);
                }
            }
        }
        GeneralView => {
            k.poly(&[(1.5, 1.5), (14.5, 1.5), (14.5, 14.5), (1.5, 14.5)], WHITE, Some(MID));
            k.iso((8.0, 13.0), 4.4, 4.4, 4.6);
        }
        ProjView => {
            k.poly(&[(1.0, 4.0), (7.0, 4.0), (7.0, 12.0), (1.0, 12.0)], YL, Some(YO));
            k.poly(&[(9.5, 4.0), (14.5, 4.0), (14.5, 12.0), (9.5, 12.0)], YR, Some(YO));
            k.dashed((7.0, 8.0), (9.5, 8.0), 0.8, BLUE, 1.0, 0.6);
        }
        DetailView => {
            k.poly(&[(1.0, 7.0), (11.0, 7.0), (11.0, 14.0), (1.0, 14.0)], YL, Some(YO));
            k.ring((10.0, 6.0), 4.5, 1.2, BLUE);
            k.path(&[(13.2, 9.2), (15.2, 11.2)], 1.6, BLUE);
        }
        SectionView => {
            k.poly(&[(2.0, 3.0), (14.0, 3.0), (14.0, 13.0), (2.0, 13.0)], WHITE, Some(DARK));
            for i in 0..5 { let x = 2.0 + i as f32 * 3.0; k.path(&[(x, 13.0), ((x + 6.0).min(14.0), 13.0 - ((x + 6.0).min(14.0) - x))], 0.8, DARK); }
            k.path(&[(1.0, 8.0), (15.0, 8.0)], 1.2, RED);
        }
        DwgDim => {
            k.path(&[(2.0, 4.0), (2.0, 14.0)], 0.9, DARK);
            k.path(&[(14.0, 4.0), (14.0, 14.0)], 0.9, DARK);
            k.arrow((8.0, 9.0), (2.4, 9.0), 1.0, BLUE, 2.6);
            k.arrow((8.0, 9.0), (13.6, 9.0), 1.0, BLUE, 2.6);
            k.text((8.0, 5.0), "25", 5.0, BLUE);
        }
        Note => {
            k.path(&[(6.0, 3.5), (14.0, 3.5)], 1.0, DARK);
            k.path(&[(6.0, 6.5), (12.0, 6.5)], 1.0, DARK);
            k.path(&[(6.0, 9.5), (13.0, 9.5)], 1.0, DARK);
            k.path(&[(5.0, 6.5), (1.5, 14.0)], 0.9, BLUE);
            k.head((1.5, 14.0), (5.0, 6.5), 2.6, BLUE);
        }
        Balloon => {
            k.circle((10.0, 6.0), 4.6, WHITE, Some((1.1, BLUE)));
            k.text((10.0, 6.2), "1", 5.6, BLUE);
            k.path(&[(6.8, 9.3), (2.0, 14.0)], 0.9, DARK);
            k.circle((2.0, 14.0), 1.0, DARK, None);
        }
        ShowAnnot => {
            k.poly(&[(1.0, 6.0), (10.0, 6.0), (10.0, 14.0), (1.0, 14.0)], YL, Some(YO));
            k.arrow((8.0, 3.0), (11.6, 3.0), 0.9, BLUE, 2.2);
            k.arrow((8.0, 3.0), (4.4, 3.0), 0.9, BLUE, 2.2);
            k.poly(&[(13.5, 5.0), (11.0, 10.0), (13.0, 10.0), (12.0, 15.0), (15.5, 8.5), (13.5, 8.5), (15.0, 5.0)], rgb(250, 196, 40), Some(rgb(170, 120, 20)));
        }
        Cleanup => {
            k.path(&[(2.0, 12.0), (14.0, 12.0)], 1.0, DARK);
            k.path(&[(2.0, 8.0), (14.0, 8.0)], 1.0, BLUE);
            k.path(&[(2.0, 4.0), (14.0, 4.0)], 1.0, BLUE);
            k.arrow((8.0, 12.0), (8.0, 9.0), 1.0, GREEN, 2.4);
        }
        GeomTol => {
            k.poly(&[(0.5, 4.5), (15.5, 4.5), (15.5, 11.5), (0.5, 11.5)], WHITE, Some(DARK));
            k.path(&[(6.0, 4.5), (6.0, 11.5)], 0.8, DARK);
            k.path(&[(11.5, 4.5), (11.5, 11.5)], 0.8, DARK);
            k.ring((3.25, 8.0), 1.6, 0.8, BLUE);
            k.path(&[(1.2, 8.0), (5.3, 8.0)], 0.7, BLUE);
            k.path(&[(3.25, 5.8), (3.25, 10.2)], 0.7, BLUE);
            k.text((8.75, 8.2), ".1", 4.2, DARK);
            k.text((13.5, 8.2), "A", 4.2, DARK);
        }
        DatumFeature => {
            k.poly(&[(4.5, 1.5), (11.5, 1.5), (11.5, 8.5), (4.5, 8.5)], WHITE, Some(DARK));
            k.text((8.0, 5.2), "A", 5.0, BLUE);
            k.path(&[(8.0, 8.5), (8.0, 11.5)], 0.9, DARK);
            k.poly(&[(5.0, 14.5), (11.0, 14.5), (8.0, 11.0)], DARK, None);
            k.path(&[(1.0, 14.8), (15.0, 14.8)], 1.0, BROWN);
        }
        SurfFinish => {
            k.path(&[(3.0, 9.0), (6.0, 14.0), (11.5, 3.0), (15.0, 3.0)], 1.2, BLUE);
            k.path(&[(3.0, 9.0), (9.0, 9.0)], 1.0, BLUE);
            k.path(&[(1.0, 14.5), (15.0, 14.5)], 1.0, BROWN);
        }
        DimTol => {
            k.path(&[(2.0, 12.5), (14.0, 12.5)], 0.9, DARK);
            k.text((6.5, 6.5), "±", 9.0, BLUE);
            k.text((12.0, 5.0), "+", 4.0, DARK);
            k.text((12.0, 9.0), "-", 4.0, DARK);
        }
        Lock => {
            k.poly(&[(3.5, 7.5), (12.5, 7.5), (12.5, 14.5), (3.5, 14.5)], YL, Some(YO));
            let a = k.arc((8.0, 7.5), 3.2, 4.2, 0.0, 180.0);
            k.path(&a, 1.5, DARK);
        }
        AsmFix => {
            k.iso((8.0, 13.0), 4.0, 4.0, 4.0);
            k.poly(&[(4.5, 14.0), (11.5, 14.0), (11.5, 15.5), (4.5, 15.5)], DARK, None);
            for x in [5.0, 7.5, 10.0] { k.path(&[(x, 15.5), (x - 1.5, 16.0)], 0.8, DARK); }
            k.path(&[(8.0, 1.0), (8.0, 5.0)], 1.0, DARK);
        }
        Copy => {
            k.poly(&[(2.0, 2.0), (9.5, 2.0), (9.5, 11.0), (2.0, 11.0)], WHITE, Some(MID));
            k.poly(&[(6.0, 5.0), (13.5, 5.0), (13.5, 14.5), (6.0, 14.5)], WHITE, Some(DARK));
            for y in [8.0, 10.0, 12.0] { k.path(&[(7.5, y), (12.0, y)], 0.7, MID); }
        }
        Paste => {
            k.poly(&[(2.5, 3.0), (11.0, 3.0), (11.0, 14.5), (2.5, 14.5)], TAN, Some(BROWN));
            k.poly(&[(5.0, 1.5), (8.5, 1.5), (8.5, 4.0), (5.0, 4.0)], LIGHT, Some(DARK));
            k.poly(&[(7.0, 7.0), (14.0, 7.0), (14.0, 15.0), (7.0, 15.0)], WHITE, Some(DARK));
        }
        GetData => {
            k.iso((8.0, 14.0), 4.0, 4.0, 4.0);
            k.arrow((14.5, 1.5), (9.5, 6.5), 1.4, BLUE, 3.2);
        }
        Body => {
            k.iso_pal((7.0, 14.5), 5.0, 4.0, 5.0, [rgb(206, 236, 190), rgb(120, 190, 90), rgb(80, 150, 60), GREEN_D]);
        }
        Rib => {
            k.poly(&[(1.0, 14.0), (15.0, 14.0), (15.0, 11.5), (1.0, 11.5)], GLEFT, Some(GLINE));
            k.poly(&[(1.0, 11.5), (3.5, 11.5), (3.5, 2.0), (1.0, 2.0)], GLEFT, Some(GLINE));
            k.poly(&[(3.5, 4.0), (3.5, 11.5), (11.0, 11.5)], YL, Some(YO));
        }
        SweptBlend => {
            let a = k.arc((14.0, 15.0), 11.0, 11.0, 95.0, 175.0);
            k.path(&a, 1.0, SK);
            k.poly(&[(1.0, 10.0), (6.0, 10.0), (6.0, 15.0), (1.0, 15.0)], YL, Some(YO));
            k.circle((12.5, 4.0), 2.6, YT, Some((1.0, YO)));
        }
        ShortEdge => {
            k.iso_pal((8.0, 14.0), 5.0, 5.0, 4.0, [GTOP, GLEFT, GRIGHT, GLINE]);
            k.path(&[(8.0, 10.0), (8.0, 14.0)], 2.2, RED);
        }
        Thickness => {
            k.poly(&[(1.0, 6.0), (15.0, 6.0), (15.0, 10.0), (1.0, 10.0)], GLEFT, Some(GLINE));
            k.arrow((8.0, 1.0), (8.0, 6.0), 1.0, RED, 2.6);
            k.arrow((8.0, 15.0), (8.0, 10.0), 1.0, RED, 2.6);
        }
        GeomReport => {
            k.poly(&[(1.5, 4.0), (14.5, 4.0), (14.5, 13.5), (1.5, 13.5)], WHITE, Some(MID));
            k.poly(&[(1.5, 4.0), (14.5, 4.0), (14.5, 6.0), (1.5, 6.0)], BLUE_L, None);
            k.arc((8.0, 13.5), 5.0, 5.0, 0.0, 180.0).windows(2).for_each(|w| k.path(w, 1.2, GREEN));
        }
        DraftCheck => {
            k.poly(&[(3.0, 14.5), (5.5, 3.0), (10.5, 3.0), (13.0, 14.5)], rgb(120, 170, 230), Some(DARK));
            k.poly(&[(5.5, 3.0), (10.5, 3.0), (11.3, 7.0), (4.7, 7.0)], rgb(240, 90, 80), None);
        }
        MeshSurf => {
            for i in 0..5 { let t = 2.0 + i as f32 * 3.0; k.path(&[(t, 2.0), (t, 14.0)], 0.8, BLUE); k.path(&[(2.0, t), (14.0, t)], 0.8, BLUE); }
        }
        DevAngle => {
            k.path(&[(1.5, 12.0), (14.5, 12.0)], 1.2, DARK);
            k.path(&[(1.5, 12.0), (13.0, 3.0)], 1.2, BLUE);
            let a = k.arc((1.5, 12.0), 7.0, 7.0, 0.0, 38.0);
            k.path(&a, 1.0, RED);
        }
        Curvature => {
            let a = k.arc((8.0, 18.0), 9.0, 9.0, 35.0, 145.0);
            k.path(&a, 1.4, DARK);
            for t in [55.0f32, 75.0, 95.0, 115.0, 135.0] { let r = t.to_radians(); k.path(&[(8.0 + 9.0 * r.cos(), 18.0 - 9.0 * r.sin()), (8.0 + 13.0 * r.cos(), 18.0 - 13.5 * r.sin())], 0.8, BLUE); }
        }
        PairClear => {
            k.circle((4.0, 8.0), 3.0, YL, Some((1.0, YO)));
            k.circle((12.0, 8.0), 3.0, BLUE_L, Some((1.0, BLUE)));
            k.arrow((8.0, 3.0), (1.5, 3.0), 0.8, DARK, 2.0);
            k.arrow((8.0, 3.0), (14.5, 3.0), 0.8, DARK, 2.0);
        }
        DimBound => {
            k.path(&[(2.0, 3.0), (2.0, 9.0)], 1.0, DARK); k.path(&[(14.0, 3.0), (14.0, 9.0)], 1.0, DARK);
            k.arrow((8.0, 6.0), (2.5, 6.0), 1.0, DARK, 2.4); k.arrow((8.0, 6.0), (13.5, 6.0), 1.0, DARK, 2.4);
            k.text((8.0, 12.0), "dl+", 6.0, DARK);
        }
        Probe | SimReport => {
            if icon == Probe {
                k.poly(&[(6.0, 1.5), (10.0, 1.5), (10.0, 9.0), (6.0, 9.0)], LIGHT, Some(MID));
                k.poly(&[(7.0, 9.0), (9.0, 9.0), (8.0, 14.5)], MID, None);
                k.circle((8.0, 14.5), 1.2, RED, None);
            } else {
                k.poly(&[(2.5, 1.5), (13.5, 1.5), (13.5, 14.5), (2.5, 14.5)], WHITE, Some(MID));
                for y in [4.0, 6.5, 9.0, 11.5] { k.path(&[(4.5, y), (11.5, y)], 0.8, MID); }
            }
        }
        FieldOfView => {
            k.poly(&[(1.5, 8.0), (14.5, 2.0), (14.5, 14.0)], rgb(230, 234, 238), Some(MID));
            k.circle((3.0, 8.0), 1.8, DARK, None);
        }
        Contact | Fixed | Displacement => {
            k.circle((5.0, 5.0), 3.5, LIGHT, Some((1.0, MID)));
            k.circle((11.0, 11.0), 3.5, LIGHT, Some((1.0, MID)));
            if icon == Fixed { k.poly(&[(2.0, 11.0), (8.0, 11.0), (8.0, 15.0), (2.0, 15.0)], LIGHT, Some(MID)); }
            if icon == Displacement { k.arrow((2.0, 14.0), (8.0, 14.0), 1.2, MID, 2.6); }
        }
        Force | Moment | Pressure | Gravity | Centrifugal | LinAccel => {
            k.poly(&[(9.0, 2.0), (14.0, 2.0), (14.0, 14.0), (9.0, 14.0)], LIGHT, Some(MID));
            match icon {
                Force => k.arrow((1.0, 8.0), (9.0, 8.0), 1.4, MID, 3.0),
                Moment => { let a = k.arc((9.0, 8.0), 5.5, 5.5, 100.0, 260.0); k.path(&a, 1.3, MID); k.head(a[a.len() - 1], a[a.len() - 3], 3.0, MID); }
                Pressure => for y in [4.0, 8.0, 12.0] { k.arrow((2.0, y), (9.0, y), 1.0, MID, 2.4) },
                Gravity => k.arrow((4.0, 2.0), (4.0, 14.0), 1.4, MID, 3.0),
                Centrifugal => { k.ring((5.0, 8.0), 3.5, 1.0, MID); k.arrow((5.0, 8.0), (1.0, 2.0), 0.9, MID, 2.2); }
                _ => { k.arrow((1.0, 6.0), (9.0, 6.0), 1.0, MID, 2.4); k.arrow((1.0, 10.0), (9.0, 10.0), 1.0, MID, 2.4); }
            }
        }
        Deform => {
            k.path(&[(1.0, 12.0), (5.0, 9.0), (10.0, 8.0), (15.0, 4.0)], 2.2, rgb(240, 120, 60));
            k.path(&[(1.0, 12.0), (15.0, 12.0)], 1.0, MID);
        }
        ComboState => {
            k.poly(&[(1.5, 5.0), (14.5, 5.0), (14.5, 14.0), (1.5, 14.0)], DARK, Some(DARK));
            k.poly(&[(5.0, 3.0), (9.0, 3.0), (10.0, 5.0), (4.0, 5.0)], DARK, None);
            k.circle((8.0, 9.5), 3.3, MID, Some((1.0, WHITE)));
            k.circle((2.8, 2.6), 2.0, rgb(250, 200, 40), None);
        }
        AnnotPlane => {
            k.poly(&[(1.0, 10.0), (4.0, 4.0), (15.0, 4.0), (12.0, 10.0)], rgb(130, 200, 240), Some(BLUE));
            k.poly(&[(4.0, 4.0), (7.0, 1.0), (15.0, 1.0), (15.0, 4.0)], rgb(80, 150, 220), None);
        }
        AnnotFeature | DatumTarget => {
            k.poly(&[(1.5, 2.5), (11.5, 2.5), (11.5, 8.5), (1.5, 8.5)], rgb(250, 240, 200), Some(MID));
            if icon == DatumTarget { k.ring((6.5, 5.5), 2.2, 1.0, DARK); }
            k.path(&[(11.5, 8.5), (14.0, 14.5)], 1.2, DARK);
            k.head((13.5, 15.0), (12.5, 11.0), 3.0, DARK);
        }
        RefViewer => {
            k.circle((4.0, 11.0), 3.0, rgb(200, 210, 220), Some((1.0, DARK)));
            k.circle((12.0, 5.0), 3.0, rgb(200, 210, 220), Some((1.0, DARK)));
            k.path(&[(6.0, 9.0), (10.0, 7.0)], 2.4, DARK);
            k.circle((13.0, 12.5), 2.8, BLUE, None);
            k.text((13.0, 12.6), "i", 4.5, WHITE);
        }
        Search => {
            k.poly(&[(1.5, 6.0), (6.5, 6.0), (6.5, 14.5), (1.5, 14.5)], DARK, None);
            k.poly(&[(9.5, 6.0), (14.5, 6.0), (14.5, 14.5), (9.5, 14.5)], DARK, None);
            k.poly(&[(6.5, 8.0), (9.5, 8.0), (9.5, 11.0), (6.5, 11.0)], MID, None);
            k.poly(&[(2.5, 2.0), (5.5, 2.0), (5.5, 6.0), (2.5, 6.0)], MID, None);
            k.poly(&[(10.5, 2.0), (13.5, 2.0), (13.5, 6.0), (10.5, 6.0)], MID, None);
        }
        CompInterface => {
            k.poly(&[(1.5, 2.0), (14.5, 2.0), (14.5, 14.0), (1.5, 14.0)], rgb(250, 236, 170), Some(YO));
            k.path(&[(4.0, 11.0), (4.0, 6.0), (8.0, 6.0), (8.0, 9.0), (12.0, 9.0), (12.0, 4.0)], 1.2, BLUE);
        }
        PublishGeom => {
            k.iso((7.0, 14.5), 4.5, 4.0, 4.5);
            k.arrow((8.0, 6.0), (14.5, 1.0), 1.3, BLUE, 3.0);
            k.path(&[(11.0, 1.5), (14.5, 1.0), (14.5, 4.5)], 1.0, BLUE);
        }
        FamilyTable => {
            k.poly(&[(1.5, 2.0), (14.5, 2.0), (14.5, 14.0), (1.5, 14.0)], WHITE, Some(MID));
            for y in [5.0, 8.0, 11.0] { k.path(&[(1.5, y), (14.5, y)], 0.7, MID); }
            for x in [5.5, 10.0] { k.path(&[(x, 2.0), (x, 14.0)], 0.7, MID); }
        }
        SwitchDims => {
            k.text((5.0, 5.0), "15", 6.0, DARK);
            k.text((11.0, 11.0), "d1", 6.0, BLUE);
            k.path(&[(3.0, 14.0), (13.0, 2.0)], 1.0, MID);
        }
        UdfLib => {
            k.iso_pal((8.0, 14.5), 4.5, 4.5, 5.0, [rgb(220, 232, 250), rgb(140, 176, 230), rgb(90, 130, 200), BLUE]);
            k.circle((12.0, 4.0), 2.5, rgb(250, 190, 40), None);
        }
        ImageEditor => { k.poly(&[(2.0, 2.0), (14.0, 2.0), (14.0, 14.0), (2.0, 14.0)], WHITE, Some(MID)); k.pencil((5.0, 13.0)); }
        Hatch => {
            k.poly(&[(2.0, 2.0), (14.0, 2.0), (14.0, 14.0), (2.0, 14.0)], WHITE, Some(MID));
            for i in 0..5 { let t = i as f32 * 3.0; k.path(&[(2.0 + t, 14.0), (2.0, 14.0 - t)], 0.7, DARK); k.path(&[(14.0, 2.0 + t), (2.0 + t + 0.0, 2.0)], 0.0, DARK); }
            k.path(&[(5.0, 14.0), (14.0, 5.0)], 0.7, DARK); k.path(&[(8.0, 14.0), (14.0, 8.0)], 0.7, DARK); k.path(&[(11.0, 14.0), (14.0, 11.0)], 0.7, DARK);
        }
        AuxApps => {
            k.poly(&[(2.0, 4.0), (14.0, 4.0), (14.0, 14.0), (2.0, 14.0)], LIGHT, Some(MID));
            k.path(&[(5.0, 9.0), (11.0, 9.0)], 1.6, BLUE); k.path(&[(8.0, 6.0), (8.0, 12.0)], 1.6, BLUE);
        }
        ArTarget => {
            for (a, b) in [((1.5, 5.0), (1.5, 1.5)), ((1.5, 1.5), (5.0, 1.5)), ((11.0, 1.5), (14.5, 1.5)), ((14.5, 1.5), (14.5, 5.0)), ((1.5, 11.0), (1.5, 14.5)), ((1.5, 14.5), (5.0, 14.5)), ((11.0, 14.5), (14.5, 14.5)), ((14.5, 14.5), (14.5, 11.0))] { k.path(&[a, b], 1.4, rgb(0, 160, 210)); }
            k.path(&[(8.0, 5.0), (8.0, 11.0)], 1.4, rgb(0, 160, 210)); k.path(&[(5.0, 8.0), (11.0, 8.0)], 1.4, rgb(0, 160, 210));
        }
        Layers => {
            for (i, c) in [(0.0, rgb(250, 200, 80)), (3.0, rgb(120, 180, 240)), (6.0, rgb(140, 210, 120))] {
                k.poly(&[(1.0, 6.0 + i), (8.0, 3.0 + i), (15.0, 6.0 + i), (8.0, 9.0 + i)], c, Some(DARK));
            }
        }
        Scene => { k.circle((8.0, 8.0), 6.5, MID, None); k.circle((6.0, 6.0), 3.0, rgb(200, 204, 210), None); k.circle((5.5, 5.5), 1.2, WHITE, None); }
        Pan => {
            k.poly(&[(5.0, 15.0), (3.0, 9.0), (4.0, 8.0), (6.0, 10.0), (6.0, 3.0), (8.0, 3.0), (8.0, 8.0), (13.0, 9.0), (12.5, 15.0)], rgb(250, 230, 200), Some(DARK));
        }
        ZoomSel => { k.magnifier(None); k.poly(&[(4.0, 4.5), (10.0, 4.5), (10.0, 9.5), (4.0, 9.5)], rgb(200, 210, 220), Some(MID)); }
        ViewBack => {
            k.iso_pal((8.0, 13.0), 4.0, 4.0, 4.0, [rgb(220, 236, 252), rgb(140, 190, 240), rgb(80, 140, 220), BLUE]);
            let a = k.arc((8.0, 8.0), 6.5, 6.5, 100.0, 200.0);
            k.path(&a, 1.4, DARK); k.head(a[a.len() - 1], a[a.len() - 3], 3.0, DARK);
        }
        Windows | ActivateWin => {
            k.poly(&[(1.5, 3.0), (11.0, 3.0), (11.0, 10.5), (1.5, 10.5)], WHITE, Some(DARK));
            k.poly(&[(5.0, 6.0), (14.5, 6.0), (14.5, 13.5), (5.0, 13.5)], WHITE, Some(DARK));
            k.poly(&[(5.0, 6.0), (14.5, 6.0), (14.5, 7.5), (5.0, 7.5)], BLUE, None);
            if icon == ActivateWin { k.path(&[(7.0, 10.5), (9.0, 12.5), (13.0, 8.5)], 1.4, GREEN); }
        }
        FlexMove | FlexOffset | FlexModify | FlexSymmetry | FlexRounds | Islands => {
            let tint = [rgb(236, 220, 246), rgb(190, 160, 220), rgb(150, 120, 190), rgb(100, 70, 140)];
            match icon {
                FlexMove => { k.iso_pal((7.0, 14.0), 4.0, 3.5, 4.0, tint); k.arrow((9.0, 6.0), (15.0, 6.0), 1.4, DARK, 3.0); }
                FlexOffset => { k.iso_pal((8.0, 14.5), 4.5, 4.5, 3.0, tint); k.iso_pal((8.0, 8.5), 4.5, 4.5, 2.5, [rgb(200, 230, 250), BLUE_L, BLUE, BLUE]); }
                FlexModify => { k.poly(&[(3.0, 4.0), (8.0, 1.5), (13.0, 4.0), (13.0, 13.0), (3.0, 13.0)], rgb(200, 230, 250), Some(BLUE)); k.pencil((9.0, 14.5)); }
                FlexSymmetry => { k.poly(&[(1.0, 3.0), (6.0, 5.0), (6.0, 14.0), (1.0, 12.0)], tint[1], Some(tint[3])); k.poly(&[(15.0, 3.0), (10.0, 5.0), (10.0, 14.0), (15.0, 12.0)], tint[1], Some(tint[3])); k.dashed((8.0, 1.0), (8.0, 15.0), 0.8, DARK, 2.0, 1.0); }
                FlexRounds => { let a = k.arc((13.0, 13.0), 9.0, 9.0, 90.0, 180.0); let mut v = a.clone(); v.push((13.0, 13.0)); k.path(&a, 2.0, tint[3]); k.path(&[(4.0, 13.0), (15.0, 13.0)], 1.2, tint[3]); }
                _ => { k.poly(&[(2.0, 4.0), (8.0, 2.0), (14.0, 4.0), (14.0, 12.0), (8.0, 14.0), (2.0, 12.0)], LIGHT, Some(MID)); }
            }
        }
        Weld => {
            k.poly(&[(1.0, 15.0), (1.0, 3.0), (5.0, 3.0), (5.0, 11.0), (15.0, 11.0), (15.0, 15.0)], LIGHT, Some(DARK));
            k.poly(&[(5.0, 7.0), (5.0, 11.0), (9.0, 11.0)], RED, None);
        }
        Mold | Moldbase => {
            k.poly(&[(2.0, 6.0), (8.0, 3.0), (14.0, 6.0), (8.0, 9.0)], rgb(170, 220, 250), Some(BLUE));
            k.poly(&[(2.0, 6.0), (8.0, 9.0), (8.0, 12.0), (2.0, 9.0)], rgb(90, 170, 230), Some(BLUE));
            k.poly(&[(8.0, 9.0), (14.0, 6.0), (14.0, 9.0), (8.0, 12.0)], rgb(60, 140, 210), Some(BLUE));
            if icon == Moldbase { k.poly(&[(3.0, 13.0), (13.0, 13.0), (13.0, 15.0), (3.0, 15.0)], LIGHT, Some(MID)); }
        }
        GenDesign => {
            let a = k.arc((8.0, 8.0), 6.0, 6.0, 40.0, 320.0);
            k.path(&a, 2.6, rgb(40, 40, 44));
            k.path(&[(8.5, 8.0), (14.0, 8.0)], 2.2, GREEN);
            k.path(&[(10.0, 3.5), (13.0, 2.0)], 2.2, GREEN);
        }
        Simulate | SimResults => {
            k.poly(&[(8.0, 1.5), (14.5, 14.0), (1.5, 14.0)], LIGHT, Some(MID));
            k.poly(&[(8.0, 5.5), (11.5, 12.0), (4.5, 12.0)], WHITE, None);
            if icon == SimResults { k.poly(&[(1.5, 9.0), (6.0, 9.0), (6.0, 14.0), (1.5, 14.0)], rgb(240, 120, 60), None); }
        }
        Flow => {
            k.circle((8.0, 8.0), 7.0, rgb(240, 250, 255), Some((1.0, BLUE)));
            for (i, c) in [rgb(230, 60, 60), rgb(250, 190, 40), rgb(60, 170, 80), rgb(40, 120, 220)].iter().enumerate() {
                let t = (i as f32 * 90.0f32).to_radians();
                k.poly(&[(8.0, 8.0), (8.0 + 6.5 * t.cos(), 8.0 - 6.5 * t.sin()), (8.0 + 6.5 * (t + 0.9).cos(), 8.0 - 6.5 * (t + 0.9).sin())], *c, None);
            }
        }
        MoldFlow => {
            k.poly(&[(1.0, 9.0), (8.0, 5.0), (15.0, 9.0), (8.0, 13.0)], rgb(240, 60, 80), None);
            k.poly(&[(1.0, 9.0), (8.0, 5.0), (8.0, 9.0)], rgb(250, 190, 40), None);
            k.poly(&[(8.0, 5.0), (15.0, 9.0), (8.0, 9.0)], rgb(60, 120, 230), None);
        }
        Mathcad => { k.text((8.0, 8.5), "M", 13.0, rgb(150, 156, 164)); k.text((13.5, 3.5), "+", 6.0, MID); }
        GdtAdvisor | EzTol => {
            if icon == GdtAdvisor { k.ring((7.0, 7.0), 4.5, 1.2, DARK); k.path(&[(7.0, 1.0), (7.0, 13.0)], 1.0, DARK); k.path(&[(1.0, 7.0), (13.0, 7.0)], 1.0, DARK); }
            else { k.text((8.0, 6.0), "±.01", 7.0, DARK); }
            k.path(&[(8.0, 13.0), (10.5, 15.0), (15.0, 9.5)], 1.8, GREEN);
        }
        Render => {
            k.circle((8.0, 9.5), 5.5, rgb(40, 140, 210), None);
            k.poly(&[(6.0, 2.0), (10.0, 2.0), (9.0, 4.5), (7.0, 4.5)], rgb(40, 140, 210), None);
            k.circle((6.0, 8.0), 1.8, rgb(170, 220, 250), None);
        }
        BoundaryBlend | FillSurf | Style | Freestyle | Extend | Trim | Merge | Intersect | Offset | Solidify | Project3d => {
            let surf = |k: &Ic| { let up = k.arc((8.0, 14.0), 7.0, 5.0, 20.0, 160.0); let mut v = vec![(1.5, 14.0)]; v.extend(up); v.push((14.5, 14.0)); k.closed(&v, 1.0, BLUE); };
            match icon {
                BoundaryBlend => { k.poly(&[(2.0, 12.0), (5.0, 3.0), (14.0, 4.0), (12.0, 13.0)], rgb(190, 225, 250), Some(BLUE)); }
                FillSurf => { k.poly(&[(2.0, 4.0), (14.0, 4.0), (14.0, 12.0), (2.0, 12.0)], rgb(190, 225, 250), Some(BLUE)); }
                Style => { k.path(&[(1.0, 12.0), (5.0, 4.0), (10.0, 10.0), (15.0, 3.0)], 1.6, BLUE); k.circle((5.0, 4.0), 1.2, RED, None); k.circle((10.0, 10.0), 1.2, RED, None); }
                Freestyle => { surf(&k); k.circle((8.0, 6.0), 1.6, rgb(160, 90, 200), None); }
                Extend => { k.poly(&[(1.0, 4.0), (9.0, 4.0), (9.0, 12.0), (1.0, 12.0)], rgb(190, 225, 250), Some(BLUE)); k.arrow((9.0, 8.0), (15.0, 8.0), 1.2, DARK, 2.6); }
                Trim => { k.poly(&[(1.0, 4.0), (15.0, 4.0), (15.0, 12.0), (1.0, 12.0)], rgb(190, 225, 250), Some(BLUE)); k.path(&[(9.0, 2.0), (6.0, 14.0)], 1.4, RED); }
                Merge => { k.poly(&[(1.0, 3.0), (10.0, 3.0), (10.0, 10.0), (1.0, 10.0)], rgb(190, 225, 250), Some(BLUE)); k.poly(&[(6.0, 6.0), (15.0, 6.0), (15.0, 13.0), (6.0, 13.0)], rgb(170, 210, 240), Some(BLUE)); }
                Intersect => { k.poly(&[(1.0, 8.0), (15.0, 8.0), (12.0, 13.0), (4.0, 13.0)], rgb(190, 225, 250), Some(BLUE)); k.poly(&[(7.0, 1.0), (10.0, 1.0), (10.0, 15.0), (7.0, 15.0)], rgb(170, 210, 240), Some(BLUE)); k.path(&[(7.0, 8.0), (10.0, 8.0)], 1.8, RED); }
                Offset => { surf(&k); let up = k.arc((8.0, 11.0), 6.0, 4.0, 20.0, 160.0); k.path(&up, 1.0, rgb(160, 90, 200)); }
                Solidify => { k.iso((8.0, 14.0), 5.0, 5.0, 4.0); k.poly(&[(1.0, 4.0), (8.0, 1.0), (15.0, 4.0), (8.0, 7.0)], rgb(190, 225, 250), Some(BLUE)); }
                _ => { k.poly(&[(1.0, 10.0), (4.0, 5.0), (15.0, 5.0), (12.0, 10.0)], rgb(190, 225, 250), Some(BLUE)); k.dashed((8.0, 1.0), (8.0, 8.0), 1.0, SK, 1.5, 1.0); }
            }
        }
        WorkDir | LastSession | Erase => {
            k.poly(&[(1.5, 3.5), (6.0, 3.5), (7.2, 5.0), (14.0, 5.0), (14.0, 13.0), (1.5, 13.0)], rgb(250, 212, 110), Some(rgb(150, 105, 20)));
            match icon {
                WorkDir => k.circle((12.5, 4.0), 2.6, rgb(60, 140, 220), None),
                LastSession => { k.circle((11.5, 9.0), 4.0, WHITE, Some((1.0, BLUE))); k.path(&[(11.5, 6.5), (11.5, 9.0), (13.5, 10.0)], 1.0, BLUE); }
                _ => { k.path(&[(9.0, 7.0), (14.0, 12.0)], 1.6, RED); k.path(&[(14.0, 7.0), (9.0, 12.0)], 1.6, RED); }
            }
        }
        Server => {
            k.poly(&[(2.0, 2.0), (14.0, 2.0), (14.0, 10.0), (2.0, 10.0)], rgb(80, 150, 220), Some(DARK));
            k.poly(&[(3.5, 3.5), (12.5, 3.5), (12.5, 8.5), (3.5, 8.5)], rgb(170, 220, 250), None);
            k.poly(&[(6.0, 10.0), (10.0, 10.0), (11.0, 13.0), (5.0, 13.0)], LIGHT, Some(MID));
            k.path(&[(3.0, 14.5), (13.0, 14.5)], 1.6, GREEN);
        }
        SysColors => {
            k.poly(&[(1.5, 1.5), (8.0, 1.5), (8.0, 8.0), (1.5, 8.0)], rgb(40, 130, 200), None);
            k.poly(&[(8.0, 1.5), (14.5, 1.5), (14.5, 8.0), (8.0, 8.0)], rgb(250, 200, 200), None);
            k.poly(&[(1.5, 8.0), (8.0, 8.0), (8.0, 14.5), (1.5, 14.5)], rgb(170, 220, 250), None);
            k.poly(&[(8.0, 8.0), (14.5, 8.0), (14.5, 14.5), (8.0, 14.5)], rgb(240, 100, 100), None);
        }
        ModelDisplay => { k.poly(&[(1.5, 9.0), (7.0, 3.0), (14.5, 5.0), (9.0, 12.0)], rgb(180, 230, 250), Some(rgb(60, 160, 220))); }
        Trail => {
            k.poly(&[(2.5, 1.5), (11.0, 1.5), (13.5, 4.0), (13.5, 14.5), (2.5, 14.5)], WHITE, Some(MID));
            k.circle((10.5, 11.0), 4.0, WHITE, Some((1.0, BLUE)));
            k.path(&[(10.5, 8.5), (10.5, 11.0), (12.5, 12.0)], 1.0, BLUE);
        }
        FullScreen => { k.poly(&[(2.0, 3.0), (14.0, 3.0), (14.0, 13.0), (2.0, 13.0)], WHITE, Some(DARK)); }
        Globe => {
            k.circle((8.0, 8.0), 6.5, rgb(90, 170, 230), Some((1.0, BLUE)));
            k.poly(&[(4.0, 4.0), (8.0, 3.5), (7.0, 8.0), (3.5, 9.5)], rgb(110, 190, 90), None);
            k.poly(&[(10.0, 9.0), (13.0, 10.0), (11.0, 13.0)], rgb(110, 190, 90), None);
        }
        BgWhite | BgStd | BgDark => {
            let c = match icon { BgWhite => WHITE, BgStd => rgb(110, 150, 190), _ => rgb(40, 44, 50) };
            k.poly(&[(1.5, 4.0), (6.5, 1.5), (14.5, 4.5), (9.5, 7.0)], c, Some(DARK));
            k.poly(&[(1.5, 4.0), (9.5, 7.0), (9.5, 14.5), (1.5, 11.5)], c, Some(DARK));
            k.poly(&[(9.5, 7.0), (14.5, 4.5), (14.5, 12.0), (9.5, 14.5)], c, Some(DARK));
        }
        Pdf3d => { k.poly(&[(2.5, 1.5), (11.0, 1.5), (13.5, 4.0), (13.5, 14.5), (2.5, 14.5)], WHITE, Some(MID)); k.text((8.0, 10.0), "3D", 6.0, RED); }
        ImportParams => { k.poly(&[(2.0, 4.0), (13.0, 4.0), (13.0, 14.0), (2.0, 14.0)], WHITE, Some(MID)); k.arrow((14.5, 1.0), (9.0, 7.0), 1.2, rgb(240, 150, 30), 3.0); }
        MassCalc => { k.poly(&[(2.0, 6.0), (14.0, 6.0), (13.0, 14.5), (3.0, 14.5)], LIGHT, Some(DARK)); k.poly(&[(5.0, 2.0), (11.0, 2.0), (11.0, 6.0), (5.0, 6.0)], MID, None); }
        Note3d => { k.poly(&[(1.5, 2.0), (14.5, 2.0), (14.5, 10.0), (1.5, 10.0)], rgb(250, 248, 230), Some(MID)); k.text((8.0, 6.0), "A", 6.5, DARK); k.path(&[(4.0, 10.0), (2.0, 15.0)], 1.0, DARK); }
        Sheetmetal => { k.poly(&[(1.0, 13.0), (1.0, 4.0), (3.0, 4.0), (3.0, 11.0), (15.0, 11.0), (15.0, 13.0)], LIGHT, Some(DARK)); }
        Collapse | ExpandAll => {
            for y in [3.0, 8.0, 13.0] { k.path(&[(5.5, y), (14.5, y)], 1.4, BLUE); k.sq((2.5, y), 1.2, DARK); }
            if icon == Collapse { k.path(&[(8.5, 5.0), (8.5, 11.0)], 0.0, DARK); }
        }
        Columns => {
            k.poly(&[(1.5, 2.0), (14.5, 2.0), (14.5, 14.0), (1.5, 14.0)], WHITE, Some(DARK));
            k.path(&[(6.0, 2.0), (6.0, 14.0)], 1.0, DARK); k.path(&[(10.5, 2.0), (10.5, 14.0)], 1.0, DARK);
            k.poly(&[(1.5, 2.0), (14.5, 2.0), (14.5, 4.5), (1.5, 4.5)], BLUE_L, Some(DARK));
        }
        Filter => { k.closed(&[(1.5, 2.0), (14.5, 2.0), (9.5, 8.0), (9.5, 14.0), (6.5, 12.5), (6.5, 8.0)], 1.1, DARK); }
        Settings => {
            for y in [3.0, 8.0, 13.0] { k.path(&[(1.5, y), (14.5, y)], 1.2, DARK); }
            k.sq((5.0, 3.0), 1.6, BLUE); k.sq((11.0, 8.0), 1.6, BLUE); k.sq((7.0, 13.0), 1.6, BLUE);
        }
        PlaneTags => { k.plane([(1.0, 13.0), (4.0, 7.0), (14.0, 7.0), (11.0, 13.0)]); k.text((8.0, 3.5), "A", 6.0, BROWN); }
        Customize => tri_icon(&k),
        HelicalSweep => {
            k.dashed((8.0, 0.8), (8.0, 15.5), 0.9, DARK, 2.4, 1.2);
            for i in 0..4 {
                let y = 2.5 + i as f32 * 3.2;
                let a = k.arc((8.0, y), 5.5, 1.6, 0.0, 180.0);
                k.path(&a, 1.6, YO);
            }
            k.poly(&[(12.0, 12.0), (15.0, 12.0), (15.0, 15.0), (12.0, 15.0)], YL, Some(YO));
        }
        RotBlend => {
            k.dashed((2.0, 15.0), (2.0, 1.0), 0.9, DARK, 2.4, 1.2);
            k.poly(&[(4.0, 10.0), (8.0, 10.0), (8.0, 14.0), (4.0, 14.0)], YL, Some(YO));
            k.circle((12.0, 4.5), 2.6, YT, Some((1.0, YO)));
            let a = k.arc((2.0, 15.0), 10.0, 10.0, 15.0, 70.0);
            k.path(&a, 1.0, BLUE);
        }
        CosmThread => {
            k.poly(&[(3.0, 2.0), (13.0, 2.0), (13.0, 14.0), (3.0, 14.0)], GLEFT, Some(GLINE));
            for i in 0..5 { let y = 3.5 + i as f32 * 2.4; k.path(&[(3.0, y), (13.0, y + 1.2)], 1.0, rgb(230, 130, 30)); }
        }
        CurvePoints => {
            let c: Vec<P> = (0..=16).map(|i| { let t = i as f32 / 16.0; (1.5 + 13.0 * t, 11.0 - 7.0 * (t * 3.14).sin()) }).collect();
            k.path(&c, 1.4, BROWN);
            for (x, y) in [(1.5, 11.0), (8.0, 4.0), (14.5, 11.0)] { k.path(&[(x - 1.6, y - 1.6), (x + 1.6, y + 1.6)], 1.2, DARK); k.path(&[(x + 1.6, y - 1.6), (x - 1.6, y + 1.6)], 1.2, DARK); }
        }
    }
}

fn tri_icon(k: &Ic) { k.poly(&[(4.0, 6.0), (12.0, 6.0), (8.0, 11.0)], DARK, None); }

/// Icon used in the model tree for a feature kind.
pub fn for_feature(k: &crate::model::features::FeatureKind) -> Icon {
    use crate::model::features::FeatureKind as F;
    match k {
        F::DatumPlane(_) => Icon::Plane,
        F::DatumAxis(_) => Icon::Axis,
        F::DatumPoint(_) => Icon::Point,
        F::Csys => Icon::Csys,
        F::Sketch(_) => Icon::Sketch,
        F::Extrude(d) => if d.remove { Icon::RemoveMaterial } else { Icon::Extrude },
        F::Revolve(_) => Icon::Revolve,
        F::Sweep(_) => Icon::Sweep,
        F::Blend(d) => if d.rotational { Icon::RotBlend } else { Icon::Blend },
        F::Hole(_) => Icon::Hole,
        F::Round(_) => Icon::Round,
        F::Chamfer(_) => Icon::Chamfer,
        F::Shell(_) => Icon::Shell,
        F::Draft(_) => Icon::Draft,
        F::Pattern(_) => Icon::Pattern,
        F::Mirror(_) => Icon::Mirror,
        F::Import(_) => Icon::GetData,
        F::DatumCsys(_) => Icon::Csys,
        F::Rib(_) => Icon::Rib,
        F::Helix(_) => Icon::HelicalSweep,
        F::Thread(_) => Icon::CosmThread,
        F::Curve(_) => Icon::CurvePoints,
    }
}
