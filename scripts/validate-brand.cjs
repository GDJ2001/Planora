// Validate the deliverable contract: references, accessible pairings, exports, and original integrity.
const fs=require('node:fs');
const path=require('node:path');
const crypto=require('node:crypto');
const assert=require('node:assert/strict');
const sharp=require('sharp');
const {tokens,flatten,resolve,css}=require('./token-utils.cjs');
const root=path.resolve(__dirname,'..'),brand=path.join(root,'assets/brand');
const ORIGINAL='31e1e20534bc8715bb782f8f211db924980b94f09220d4e3c123c7b993f0ab49';
function files(dir){return fs.readdirSync(dir,{withFileTypes:true}).flatMap(e=>e.isDirectory()?files(path.join(dir,e.name)):[path.join(dir,e.name)]);}
function luminance(hex){const rgb=hex.slice(1).match(/../g).map(s=>parseInt(s,16)/255).map(n=>n<=.04045?n/12.92:((n+.055)/1.055)**2.4);return rgb[0]*.2126+rgb[1]*.7152+rgb[2]*.0722;}
function contrast(a,b){const x=luminance(a),y=luminance(b);return(Math.max(x,y)+.05)/(Math.min(x,y)+.05);}
(async()=>{
 assert.equal(crypto.createHash('sha256').update(fs.readFileSync(path.join(brand,'Planora Blue Orbit Logo.png'))).digest('hex'),ORIGINAL,'Original reference changed');
 assert.equal(tokens.version,1);assert.deepEqual(Object.keys(tokens.semantic).sort(),['dark','light']);
 assert.deepEqual(Object.keys(flatten(tokens.semantic.dark)).sort(),Object.keys(flatten(tokens.semantic.light)).sort());
 for(const value of Object.values(flatten(tokens.primitive.color)))assert.match(value,/^#[0-9A-F]{6}$/);
 const contrastResults=[];
 for(const theme of ['dark','light']){
   for(const value of Object.values({...flatten(tokens.semantic[theme]),...flatten(tokens.component)}))resolve(value,theme);
   const s=Object.fromEntries(Object.entries(flatten(tokens.semantic[theme])).map(([k,v])=>[k,resolve(v,theme)]));
   const pairs=[];
   for(const bg of ['background','surface','surfaceRaised'])for(const fg of ['text','textSecondary','accent','success','warning','error'])pairs.push([fg,bg,4.5]);
   for(const bg of ['primary','primaryHover','primaryPressed'])pairs.push(['onPrimary',bg,4.5]);
   pairs.push(['selectedText','selected',4.5],['disabledText','disabledBackground',4.5]);
   for(const bg of ['background','surface','surfaceRaised'])pairs.push(['focus',bg,3],['borderControl',bg,3]);
   for(const key of Object.keys(tokens.semantic[theme].resource))pairs.push([`resource.${key}`,'surface',3]);
   for(const [fg,bg,min] of pairs){const ratio=contrast(s[fg],s[bg]);assert(ratio>=min,`${theme} ${fg}/${bg}: ${ratio.toFixed(2)} < ${min}`);contrastResults.push({theme,foreground:fg,background:bg,ratio:Number(ratio.toFixed(2)),minimum:min});}
 }
 const all=files(brand);const svgs=all.filter(p=>p.endsWith('.svg'));
 for(const file of svgs){const source=fs.readFileSync(file,'utf8');assert(!/<(?:text|image|script)\b/i.test(source),`SVG must be vector-only: ${file}`);assert(!/(?:href|src)="https?:/i.test(source),`External SVG dependency: ${file}`);await sharp(Buffer.from(source)).metadata();}
 const pngs=all.filter(p=>p.endsWith('.png')&&!p.endsWith('Planora Blue Orbit Logo.png'));
 for(const file of pngs){const m=await sharp(file).metadata();assert.equal(m.hasAlpha,true,file);assert(m.width>0&&m.height>0);const n=/planora-(?:tray-(?:dark|light)-)?(\d+)\.png$/.exec(file);if(n){assert.equal(m.width,Number(n[1]),file);assert.equal(m.height,Number(n[1]),file);}const mac=/icon_(\d+)x\d+(@2x)?\.png$/.exec(file);if(mac){const size=Number(mac[1])*(mac[2]?2:1);assert.equal(m.width,size,file);assert.equal(m.height,size,file);}}
 const html=fs.readFileSync(path.join(brand,'showcase.html'),'utf8');assert(!/\{\{[A-Z_]+\}\}/.test(html));assert(!/<(?:link|script|img)[^>]+(?:src|href)="https?:/i.test(html),'Showcase must work offline');
 for(const match of html.matchAll(/<img\b[^>]*src="([^"]+)"/g))assert(match[1].startsWith('data:image/'),'Showcase image must be embedded');
 assert.equal(fs.readFileSync(path.join(root,'assets/design-tokens.css'),'utf8'),css(),'Generated CSS is stale');
 for(const doc of ['README.md','docs/brand-guidelines.md']){const file=path.join(root,doc);for(const match of fs.readFileSync(file,'utf8').matchAll(/\]\(([^)]+)\)/g)){if(/^(https?:|#)/.test(match[1]))continue;assert(fs.existsSync(path.resolve(path.dirname(file),decodeURIComponent(match[1].split('#')[0]))),`Broken link ${doc}: ${match[1]}`);}}
 fs.mkdirSync(path.join(root,'state'),{recursive:true});fs.writeFileSync(path.join(root,'state/brand-validation.json'),JSON.stringify({originalHash:ORIGINAL,svgFiles:svgs.length,pngFiles:pngs.length,contrast:contrastResults},null,2)+'\n');
 console.log(`PASS: original SHA-256, ${svgs.length} vector files, ${pngs.length} PNGs, ${contrastResults.length} contrast pairs, token references, offline assets, and document links.`);
})().catch(e=>{console.error(e);process.exit(1);});
