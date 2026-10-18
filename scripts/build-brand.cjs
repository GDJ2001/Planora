// Deterministic vector composition and raster exports. Original reference is never written.
const fs = require('node:fs');
const path = require('node:path');
const sharp = require('sharp');
const root = path.resolve(__dirname, '..');
const base = path.join(root, 'assets/brand');
const read = p => fs.readFileSync(path.join(base, p), 'utf8');
const write = (p, data) => { const dest = path.join(base, p); fs.mkdirSync(path.dirname(dest), {recursive:true}); fs.writeFileSync(dest, data); };
const inner = s => s.replace(/^[\s\S]*?<svg[^>]*>/, '').replace(/<\/svg>\s*$/, '').replace(/<title[\s\S]*?<\/title>/g, '');
const mark = inner(read('masters/planora-mark.svg'));
const word = inner(read('masters/planora-wordmark.svg'));
const svg = (w,h,body,title) => `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="0 0 ${w} ${h}" role="img" aria-label="${title}">${body}</svg>\n`;
const nest = (body,x,y,w,h,view) => `<svg x="${x}" y="${y}" width="${w}" height="${h}" viewBox="${view}">${body}</svg>`;
const mono = (body,color) => body.replace(/<defs>[\s\S]*?<\/defs>/g,'').replace(/fill="(?:url\([^)]+\)|#[0-9a-f]+)"/gi,`fill="${color}"`);
const wordIn = color => word.replaceAll('#05255D',color);
const small = `<path d="M5 13C2 7 7 2 12 2C18 2 22 6 22 11C22 15 19 17 17 19C15 17 15 16 17 13C19 9 16 6 13 6C10 6 9 8 9 10ZM13 11L14 12C10 14 8 15 9 17L13 21L10 24C5 20 2 17 5 14Z"/><circle cx="14" cy="9" r="2"/>`;
const iconPaths = {
 overview:'<rect x="3" y="3" width="7" height="7" rx="1.5"/><rect x="14" y="3" width="7" height="7" rx="1.5"/><rect x="3" y="14" width="7" height="7" rx="1.5"/><rect x="14" y="14" width="7" height="7" rx="1.5"/>',
 processes:'<path d="M9 5h12M9 12h12M9 19h12"/><circle cx="4" cy="5" r="1"/><circle cx="4" cy="12" r="1"/><circle cx="4" cy="19" r="1"/>',
 cpu:'<rect x="6" y="6" width="12" height="12" rx="2"/><path d="M9 3v3m6-3v3M9 18v3m6-3v3M3 9h3m-3 6h3m12-6h3m-3 6h3"/><rect x="9" y="9" width="6" height="6" rx="1"/>',
 memory:'<rect x="3" y="6" width="18" height="12" rx="2"/><path d="M7 9v5m5-5v5m5-5v5M7 18v3m5-3v3m5-3v3"/>',
 gpu:'<rect x="3" y="6" width="17" height="12" rx="2"/><circle cx="11" cy="12" r="3"/><path d="M20 9h2m-2 6h2M6 18v3m4-3v3m4-3v3"/>',
 disk:'<path d="M5 5h14l3 11v3H2v-3Z"/><path d="M2 15h20m-5 2h2M8 9h8"/>',
 network:'<rect x="9" y="2" width="6" height="6" rx="1"/><rect x="2" y="16" width="6" height="6" rx="1"/><rect x="16" y="16" width="6" height="6" rx="1"/><path d="M12 8v4H5v4m7-4h7v4"/>',
 users:'<circle cx="9" cy="7" r="3"/><path d="M3 21v-3a6 6 0 0 1 12 0v3M16 4a3 3 0 0 1 0 6m2 4a5 5 0 0 1 3 5v2"/>',
 settings:'<path d="m10 3 4 0 .5 3 2 .9L19 5l2 3-2.2 2 .2 2-.2 2L21 16l-2 3-2.5-1.9-2 .9-.5 3h-4l-.5-3-2-.9L5 19l-2-3 2.2-2-.2-2 .2-2L3 8l2-3 2.5 1.9 2-.9Z"/><circle cx="12" cy="12" r="3"/>',
 search:'<circle cx="10.5" cy="10.5" r="6.5"/><path d="m16 16 5 5"/>',
 refresh:'<path d="M20 7a9 9 0 0 0-15-2L2 8m0-5v5h5m-3 9a9 9 0 0 0 15 2l3-3m0 5v-5h-5"/>',
 pause:'<path d="M8 4v16M16 4v16"/>',
 'chevron-left':'<path d="m15 5-7 7 7 7"/>',
 'chevron-right':'<path d="m9 5 7 7-7 7"/>',
 'chevron-down':'<path d="m5 9 7 7 7-7"/>',
 check:'<path d="m5 12 4 4L19 6"/>',
 warning:'<path d="m12 3 10 18H2ZM12 9v5m0 3v.1"/>',
 error:'<circle cx="12" cy="12" r="9"/><path d="m9 9 6 6m0-6-6 6"/>'
};
async function png(name,source,width){const dest=path.join(base,name);fs.mkdirSync(path.dirname(dest),{recursive:true});await sharp(Buffer.from(source)).resize({width}).png().toFile(dest);}
(async()=>{
 const logos={};
 for (const [suffix,color] of [['light','#05255D'],['dark','#E6EDF7']]) {
   logos[`horizontal-${suffix}`]=svg(1200,400,nest(mark,20,20,300,360,'0 0 300 360')+nest(wordIn(color),370,112,800,176,'0 0 510 112'),'Planora');
   logos[`stacked-${suffix}`]=svg(760,620,nest(mark,230,20,300,360,'0 0 300 360')+nest(wordIn(color),80,442,600,132,'0 0 510 112'),'Planora');
   logos[`wordmark-${suffix}`]=svg(550,152,nest(wordIn(color),20,20,510,112,'0 0 510 112'),'Planora');
 }
 logos.mark=svg(300,360,mark,'Planora orbit mark');
 for(const [suffix,color] of [['navy','#05255D'],['white','#FFFFFF']]){
   logos[`monochrome-${suffix}`]=svg(1200,400,nest(mono(mark,color),20,20,300,360,'0 0 300 360')+nest(wordIn(color),370,112,800,176,'0 0 510 112'),'Planora');
   logos[`mark-${suffix}`]=svg(300,360,mono(mark,color),'Planora orbit mark');
 }
 for(const [name,source] of Object.entries(logos)){write(`logos/planora-${name}.svg`,source);await png(`png/planora-${name}.png`,source,name.startsWith('horizontal')||name.startsWith('monochrome')?2400:1200);}
 const fullIcon=svg(1024,1024,nest(mark,172,104,680,816,'0 0 300 360'),'Planora application icon');
 const smallIcon=svg(32,32,`<g transform="translate(4 3)" fill="#087EFF">${small}</g>`,'Planora small application icon');
 write('masters/planora-small-mark.svg',smallIcon);
 write('platforms/linux/planora.svg',fullIcon);
 const macIcon=svg(1024,1024,'<rect x="100" y="100" width="824" height="824" rx="184" fill="#0B1220"/>'+nest(mark,212,152,600,720,'0 0 300 360'),'Planora macOS icon');
 write('masters/planora-app-icon.svg',fullIcon);write('masters/planora-app-icon-macos.svg',macIcon);
 for(const size of [16,20,24,32,40,48,64,128,256])await png(`platforms/windows/planora-${size}.png`,size<=32?smallIcon:fullIcon,size);
 for(const size of [16,24,32,48,64,128,256,512])await png(`platforms/linux/planora-${size}.png`,size<=32?smallIcon:fullIcon,size);
 for(const size of [16,32,128,256,512])for(const scale of [1,2]){
   // Small platform tile retains the same footprint but uses a stronger single-color mark.
   const macSmall=svg(32,32,'<rect x="3" y="3" width="26" height="26" rx="6" fill="#0B1220"/>'+`<g transform="translate(6.4 5.6) scale(.8)" fill="#60A5FA">${small}</g>`,'Planora macOS icon');
   await png(`platforms/macos/Planora.iconset/icon_${size}x${size}${scale===2?'@2x':''}.png`,size*scale<=32?macSmall:macIcon,size*scale);
 }
 for(const [name,color] of [['dark','#14213D'],['light','#FFFFFF']]){
   const source=svg(28,28,`<g transform="translate(2 1)" fill="${color}">${small}</g>`,'Planora tray icon');write(`platforms/tray/planora-tray-${name}.svg`,source);
   for(const size of [16,20,22,24,32,44,48,64])await png(`platforms/tray/planora-tray-${name}-${size}.png`,source,size);
 }
 for(const [name,body] of Object.entries(iconPaths))write(`ui/planora-${name}.svg`,svg(24,24,`<g fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round">${body}</g>`,name.replaceAll('-',' ')));
 console.log(`Built ${Object.keys(logos).length} logo variants, 18 UI icons, and platform PNGs.`);
})().catch(e=>{console.error(e);process.exit(1);});
