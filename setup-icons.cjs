const fs = require("fs");
const path = require("path");

const iconsDir = path.join(__dirname, "src-tauri", "icons");
const rootIco = path.join(__dirname, "deepseek.ico");

if (!fs.existsSync(iconsDir)) {
    fs.mkdirSync(iconsDir, { recursive: true });
}

if (fs.existsSync(rootIco)) {
    const targetIcon = path.join(iconsDir, "icon.ico");
    fs.copyFileSync(rootIco, targetIcon);
    console.log("✅ 已复制 deepseek.ico -> src-tauri/icons/icon.ico");
} else {
    console.log("⚠️  未找到 deepseek.ico，请手动复制到 src-tauri/icons/ 目录");
}

console.log("\n📋 生成其他尺寸图标:");
console.log("   npx tauri icon deepseek.ico");
console.log("   或: tauri icon deepseek.ico");
console.log("\n📁 图标目录结构:");
console.log("   src-tauri/icons/");
console.log("   ├── icon.ico      (Windows - 已复制)");
console.log("   ├── icon.icns     (macOS - 需生成)");
console.log("   ├── 32x32.png     (需生成)");
console.log("   ├── 128x128.png   (需生成)");
console.log("   └── 128x128@2x.png (需生成)");