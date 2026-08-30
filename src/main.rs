use eframe::egui;
use gdal::{
    vector::{Geometry as GdalGeometry, LayerAccess},
    Dataset,
};
use std::collections::HashMap;
use std::path::Path;

// =========================================================================
// 1. NATO AML 3.0 六大產品規格 (STANAG 4564) 定義與幾何結構
// =========================================================================
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AmlProductType {
    CLB, ESB, LWM, MBO, RAL, SBP, UNKNOWN,
}

impl AmlProductType {
    pub fn from_string(name: &str) -> Self {
        match name {
            "CLB" => AmlProductType::CLB,
            "ESB" => AmlProductType::ESB,
            "LWM" => AmlProductType::LWM,
            "MBO" => AmlProductType::MBO,
            "RAL" => AmlProductType::RAL,
            "SBP" => AmlProductType::SBP,
            _ => AmlProductType::UNKNOWN,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Geometry {
    pub lines: Vec<Vec<(f64, f64)>>, 
}

#[derive(Debug, Clone)]
pub struct AmlFeature {
    pub feature_id: String,
    pub product: AmlProductType,
    pub object_code: String,
    pub geometry: Geometry,
    pub attributes: HashMap<String, String>,
}

// =========================================================================
// 2. 專屬軍事符號學引擎 (MIL-STD-2525D / IHO S-52)
// =========================================================================
#[derive(Debug, Clone)]
pub struct MilitarySymbolStyle {
    pub symbol_id: String,
    pub stroke_color_rgba: [u8; 4],
    pub fill_color_rgba: [u8; 4],
    pub line_width: f32,
}

pub struct SymbologyEngine {
    lookup_table: HashMap<String, MilitarySymbolStyle>,
}

impl SymbologyEngine {
    pub fn new() -> Self {
        let mut lookup_table = HashMap::new();

        lookup_table.insert("DEPCNT".to_string(), MilitarySymbolStyle {
            symbol_id: "S52_DEPCNT_LINE".to_string(),
            stroke_color_rgba: [0, 150, 255, 120],
            fill_color_rgba: [0, 0, 0, 0], 
            line_width: 0.15,
        });

        lookup_table.insert("COALNE".to_string(), MilitarySymbolStyle {
            symbol_id: "S52_COALNE".to_string(),
            stroke_color_rgba: [139, 69, 19, 255], 
            fill_color_rgba: [0, 0, 0, 0], 
            line_width: 1.5,
        });

        lookup_table.insert("LNDARE".to_string(), MilitarySymbolStyle {
            symbol_id: "S52_LNDARE".to_string(),
            stroke_color_rgba: [100, 150, 100, 255],
            fill_color_rgba: [143, 188, 143, 80], 
            line_width: 1.0,
        });

        lookup_table.insert("SEAARE".to_string(), MilitarySymbolStyle {
            symbol_id: "S52_SEAARE".to_string(),
            stroke_color_rgba: [70, 130, 180, 150],
            fill_color_rgba: [135, 206, 235, 30], 
            line_width: 0.5,
        });

        lookup_table.insert("UWTROC".to_string(), MilitarySymbolStyle {
            symbol_id: "S52_UWTROC".to_string(),
            stroke_color_rgba: [255, 69, 0, 255], 
            fill_color_rgba: [255, 69, 0, 80],
            line_width: 1.5,
        });

        lookup_table.insert("SBDARE".to_string(), MilitarySymbolStyle {
            symbol_id: "MIL2525_BEACH_SAND".to_string(),
            stroke_color_rgba: [218, 165, 32, 255], fill_color_rgba: [238, 232, 170, 100], line_width: 1.0,
        });
        lookup_table.insert("WRECKS".to_string(), MilitarySymbolStyle {
            symbol_id: "MIL2525_WRECK".to_string(),
            stroke_color_rgba: [255, 50, 50, 255], fill_color_rgba: [255, 0, 0, 80], line_width: 2.0,
        });

        Self { lookup_table }
    }

    pub fn resolve_style(&self, object_code: &str) -> MilitarySymbolStyle {
        self.lookup_table.get(object_code).cloned().unwrap_or(MilitarySymbolStyle {
            symbol_id: format!("GENERIC_{}", object_code),
            stroke_color_rgba: [0, 255, 200, 150],
            fill_color_rgba: [0, 255, 200, 20], 
            line_width: 0.8,
        })
    }
}

// =========================================================================
// 3. AML 圖資顯示核心管道 (GDAL 解析)
// =========================================================================
pub struct AmlDataViewer {
    features: Vec<AmlFeature>,
    symbology: SymbologyEngine,
    bbox_min: (f64, f64),
    bbox_max: (f64, f64),
}

impl AmlDataViewer {
    pub fn new() -> Self {
        Self {
            features: Vec::new(),
            symbology: SymbologyEngine::new(),
            bbox_min: (f64::MAX, f64::MAX),
            bbox_max: (f64::MIN, f64::MIN),
        }
    }

    pub fn load_from_s57_file(&mut self, file_path: &str, product_hint: &str) -> Result<usize, Box<dyn std::error::Error>> {
        gdal::config::set_config_option("OGR_S57_OPTIONS", "RETURN_PRIMITIVES=OFF")?;

        let dataset = Dataset::open(Path::new(file_path))?;
        let mut loaded_count = 0;
        let product_type = AmlProductType::from_string(product_hint);

        for mut layer in dataset.layers() {
            let object_code = layer.name();
            let mut field_names = Vec::new();
            for field_defn in layer.defn().fields() {
                field_names.push(field_defn.name());
            }

            for feature in layer.features() {
                if let Some(geom) = feature.geometry() {
                    let mut lines = Vec::new();
                    Self::extract_lines(geom, &mut lines);

                    for line in &lines {
                        for &(lon, lat) in line {
                            if lon < self.bbox_min.0 { self.bbox_min.0 = lon; }
                            if lon > self.bbox_max.0 { self.bbox_max.0 = lon; }
                            if lat < self.bbox_min.1 { self.bbox_min.1 = lat; }
                            if lat > self.bbox_max.1 { self.bbox_max.1 = lat; }
                        }
                    }

                    let mut attributes = HashMap::new();
                    for (i, field_name) in field_names.iter().enumerate() {
                        if let Ok(Some(val_str)) = feature.field_as_string(i) {
                            if !val_str.trim().is_empty() {
                                attributes.insert(field_name.clone(), val_str);
                            }
                        }
                    }

                    if !lines.is_empty() {
                        self.features.push(AmlFeature {
                            feature_id: format!("{}-{}", object_code, feature.fid().unwrap_or(0)),
                            product: product_type.clone(),
                            object_code: object_code.clone(),
                            geometry: Geometry { lines },
                            attributes,
                        });
                        loaded_count += 1;
                    }
                }
            }
        }
        Ok(loaded_count)
    }

    fn extract_lines(geom: &GdalGeometry, lines: &mut Vec<Vec<(f64, f64)>>) {
        let point_count = geom.point_count();
        if point_count > 0 {
            let mut line = Vec::with_capacity(point_count);
            for i in 0..point_count {
                let (x, y, _z) = geom.get_point(i as i32);
                line.push((x, y));
            }
            lines.push(line);
        }
        
        for i in 0..geom.geometry_count() {
            let sub = geom.get_geometry(i);
            Self::extract_lines(&sub, lines);
        }
    }
}

// =========================================================================
// 4. GUI 圖形介面引擎 (基於 egui) 與視窗狀態管理
// =========================================================================
struct AmlApp {
    viewer: AmlDataViewer,
    zoom: f32,
    pan: egui::Vec2,
    pan_velocity: egui::Vec2,
    clicked_coord: Option<(f64, f64)>, // 【新增】記錄使用者點擊的經緯度 (Lon, Lat)
}

impl AmlApp {
    pub fn new(viewer: AmlDataViewer) -> Self {
        Self {
            viewer,
            zoom: 1.0,
            pan: egui::vec2(0.0, 0.0),
            pan_velocity: egui::vec2(0.0, 0.0),
            clicked_coord: None,
        }
    }
}

impl eframe::App for AmlApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut frame = egui::Frame::central_panel(&ctx.style());
        frame.fill = egui::Color32::from_rgb(15, 20, 25); 

        egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading(egui::RichText::new("NATO AML 3.0 S-57 Tactical Viewer By Sejima Kyuzo").color(egui::Color32::WHITE));
                ui.label(format!(" | Features loaded: {}", self.viewer.features.len()));
                
                if ui.button("🏠 Reset View").clicked() {
                    self.zoom = 1.0;
                    self.pan = egui::vec2(0.0, 0.0);
                    self.pan_velocity = egui::vec2(0.0, 0.0);
                    self.clicked_coord = None; // 【重置】同步清除選取的點擊座標
                }
            });
            ui.separator();

            let (response, painter) = ui.allocate_painter(ui.available_size(), egui::Sense::click_and_drag());
            let rect = response.rect;

            // --- 物理平移與連續縮放控制 ---
            if response.dragged() {
                let delta = response.drag_delta();
                self.pan += delta;
                self.pan_velocity = self.pan_velocity * 0.3 + delta * 0.7;
            } else {
                if self.pan_velocity.length_sq() > 0.001 {
                    self.pan += self.pan_velocity;
                    self.pan_velocity *= 0.91;
                    ui.ctx().request_repaint();
                } else {
                    self.pan_velocity = egui::Vec2::ZERO;
                }
            }

            if response.hovered() {
                let scroll_delta = ctx.input(|i| i.raw_scroll_delta.y);
                if scroll_delta != 0.0 {
                    let sensitivity = 0.005; 
                    self.zoom *= (scroll_delta * sensitivity).exp();
                }

                let zoom_gesture = ctx.input(|i| i.zoom_delta());
                if zoom_gesture != 1.0 {
                    self.zoom *= zoom_gesture;
                }
                
                self.zoom = self.zoom.clamp(0.001, 1000.0);
            }

            // --- 繪製圖資 ---
            let (min_lon, min_lat) = self.viewer.bbox_min;
            let (max_lon, max_lat) = self.viewer.bbox_max;
            
            if min_lon < max_lon && min_lat < max_lat {
                let map_width = max_lon - min_lon;
                let map_height = max_lat - min_lat;
                
                let scale_x = rect.width() / map_width as f32;
                let scale_y = rect.height() / map_height as f32;
                let base_scale = scale_x.min(scale_y) * 0.9; 

                let center_lon = (min_lon + max_lon) / 2.0;
                let center_lat = (min_lat + max_lat) / 2.0;
                let screen_center = rect.center() + self.pan;

                // =========================================================
                // 【新增】點擊感應與逆向座標計算
                // =========================================================
                if response.clicked() {
                    if let Some(mouse_pos) = response.interact_pointer_pos() {
                        let scale = base_scale * self.zoom;
                        if scale > 0.0 {
                            let dx = mouse_pos.x - screen_center.x;
                            let dy = mouse_pos.y - screen_center.y;
                            
                            // 將螢幕位移逆向求出地圖真實經緯度
                            let lon = center_lon + (dx / scale) as f64;
                            let lat = center_lat - (dy / scale) as f64;
                            self.clicked_coord = Some((lon, lat));
                        }
                    }
                }

                let mut labels_to_draw = Vec::new();

                for feat in &self.viewer.features {
                    let style = self.viewer.symbology.resolve_style(&feat.object_code);
                    let color = egui::Color32::from_rgba_unmultiplied(
                        style.stroke_color_rgba[0], style.stroke_color_rgba[1],
                        style.stroke_color_rgba[2], style.stroke_color_rgba[3],
                    );
                    
                    let stroke = egui::Stroke::new((style.line_width * self.zoom).clamp(0.1, 1.5), color);

                    for line in &feat.geometry.lines {
                        let points: Vec<egui::Pos2> = line.iter().map(|&(lon, lat)| {
                            let dx = (lon - center_lon) as f32 * base_scale * self.zoom;
                            let dy = (center_lat - lat) as f32 * base_scale * self.zoom; 
                            screen_center + egui::vec2(dx, dy)
                        }).collect();

                        let mid_pt = if points.len() > 2 {
                            Some(points[points.len() / 2])
                        } else {
                            None
                        };

                        painter.add(egui::Shape::line(points, stroke));

                        if feat.object_code == "DEPCNT" {
                            if let Some(depth_val) = feat.attributes.get("VALDCO") {
                                if let Some(pos) = mid_pt {
                                    if self.zoom > 1.5 {
                                        labels_to_draw.push((pos, depth_val.clone()));
                                    }
                                }
                            }
                        }
                    }
                }

                // 繪製水深標籤
                for (pos, depth_val) in labels_to_draw {
                    let galley = ui.painter().layout_no_wrap(
                        depth_val,
                        egui::FontId::proportional(15.0),
                        egui::Color32::YELLOW,
                    );

                    let text_rect = egui::Align2::CENTER_CENTER.anchor_rect(
                        egui::Rect::from_center_size(pos, galley.size())
                    );

                    painter.rect_filled(
                        text_rect.expand(2.0),
                        2.0,
                        egui::Color32::from_black_alpha(200),
                    );

                    painter.galley(text_rect.min, galley, egui::Color32::YELLOW);
                }

                // =========================================================
                // 【新增】在地圖最上層繪製點擊準星標記與座標資訊
                // =========================================================
                if let Some((lon, lat)) = self.clicked_coord {
                    let dx = (lon - center_lon) as f32 * base_scale * self.zoom;
                    let dy = (center_lat - lat) as f32 * base_scale * self.zoom;
                    let target_pos = screen_center + egui::vec2(dx, dy);

                    // 1. 繪製紅色準星標示
                    painter.circle_stroke(target_pos, 6.0, egui::Stroke::new(1.5_f32, egui::Color32::RED));
                    painter.line_segment([target_pos - egui::vec2(10.0, 0.0), target_pos + egui::vec2(10.0, 0.0)], egui::Stroke::new(1.0_f32, egui::Color32::RED));
                    painter.line_segment([target_pos - egui::vec2(0.0, 10.0), target_pos + egui::vec2(0.0, 10.0)], egui::Stroke::new(1.0_f32, egui::Color32::RED));

                    // 2. 建立座標格式化字串 (精確度至小數點後 5 位)
                    let coord_text = format!("Lon: {:.5}°, Lat: {:.5}°", lon, lat);
                    let galley = ui.painter().layout_no_wrap(
                        coord_text,
                        egui::FontId::proportional(13.0),
                        egui::Color32::WHITE,
                    );

                    // 3. 畫出自適應外框浮籤
                    let label_offset = target_pos + egui::vec2(12.0, -20.0);
                    let text_rect = egui::Rect::from_min_size(label_offset, galley.size());

                    painter.rect_filled(text_rect.expand(4.0), 3.0, egui::Color32::from_black_alpha(230));
                    painter.rect_stroke(text_rect.expand(4.0), 3.0, egui::Stroke::new(1.0_f32, egui::Color32::RED));
                    painter.galley(label_offset, galley, egui::Color32::WHITE);
                }
                
                ui.ctx().request_repaint(); 
            } else {
                ui.label(egui::RichText::new("⚠️ 圖資範圍異常或無可用幾何資料").color(egui::Color32::RED).size(20.0));
            }
        });

        // =========================================================================
        // 5. 圖控 UI 元件 (修正圖示相容性與寬度鎖定)
        // =========================================================================
        egui::Area::new(egui::Id::new("map_controls_overlay"))
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-20.0, -20.0))
            .show(ctx, |ui| {
                let mut control_frame = egui::Frame::window(&ctx.style());
                control_frame.fill = egui::Color32::from_black_alpha(220);
                control_frame.rounding = egui::Rounding::same(8.0);
                control_frame.stroke = egui::Stroke::new(1.0_f32, egui::Color32::from_white_alpha(40));

                control_frame.show(ui, |ui| {
                    ui.set_width(102.0); // 鎖定尺寸

                    ui.vertical_centered(|ui| {
                        egui::Grid::new("dpad_grid")
                            .spacing(egui::vec2(2.0, 2.0))
                            .min_col_width(28.0)
                            .show(ui, |ui| {
                                ui.label("");
                                if ui.button("⬆").on_hover_text("向上平移").clicked() {
                                    self.pan.y += 60.0;
                                }
                                ui.label("");
                                ui.end_row();

                                if ui.button("⬅").on_hover_text("向左平移").clicked() {
                                    self.pan.x += 60.0;
                                }
                                if ui.button("🏠").on_hover_text("重置視角").clicked() {
                                    self.zoom = 1.0;
                                    self.pan = egui::vec2(0.0, 0.0);
                                    self.pan_velocity = egui::vec2(0.0, 0.0);
                                    self.clicked_coord = None;
                                }
                                if ui.button("➡").on_hover_text("向右平移").clicked() {
                                    self.pan.x -= 60.0;
                                }
                                ui.end_row();

                                ui.label("");
                                if ui.button("⬇").on_hover_text("向下平移").clicked() {
                                    self.pan.y -= 60.0;
                                }
                                ui.label("");
                                ui.end_row();
                            });

                        ui.add_space(6.0);

                        ui.horizontal(|ui| {
                            if ui.button(" ➕ ").on_hover_text("放大地圖").clicked() {
                                self.zoom = (self.zoom * 1.25).clamp(0.001, 1000.0);
                            }
                            if ui.button(" ➖ ").on_hover_text("縮大地圖").clicked() {
                                self.zoom = (self.zoom * 0.8).clamp(0.001, 1000.0);
                            }
                        });
                    });
                });
            });
    }
}

// =========================================================================
// 6. 系統進入點
// =========================================================================
fn main() -> eframe::Result<()> {
    let mut viewer = AmlDataViewer::new();
    let target_file = "sample_aml_data.000"; 

    println!("正在啟動底層 GDAL 引擎解析 AML 圖資，請稍候...");
    
    if let Err(e) = viewer.load_from_s57_file(target_file, "LWM") {
        eprintln!("❌ 讀取檔案失敗: {} (請確認檔案路徑與 GDAL DLL 放置位置正確)", e);
        return Ok(());
    }
    println!("✅ 解析完成！正在啟動戰情圖台...");

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 800.0])
            .with_title("NATO AML 3.0 S-57 Tactical Viewer By Sejima Kyuzo"),
        ..Default::default()
    };

    eframe::run_native(
        "AML Tactical Viewer",
        options,
        Box::new(|_cc| Box::new(AmlApp::new(viewer))),
    )
}