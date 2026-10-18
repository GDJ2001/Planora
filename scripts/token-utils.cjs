const fs=require('node:fs');
const path=require('node:path');
const tokens=JSON.parse(fs.readFileSync(path.join(__dirname,'../assets/design-tokens.json'),'utf8'));
const kebab=s=>s.replace(/[A-Z]/g,c=>'-'+c.toLowerCase()).replaceAll('.','-');
function flatten(o,prefix='',out={}){for(const [k,v] of Object.entries(o)){const p=prefix?`${prefix}.${k}`:k;if(v&&typeof v==='object'&&!Array.isArray(v))flatten(v,p,out);else out[p]=v;}return out;}
function resolve(value,theme,seen=[]){
 if(typeof value!=='string'||!value.startsWith('{'))return value;
 const match=/^\{(primitive|semantic)\.([\w.]+)\}$/.exec(value);
 if(!match)throw Error(`Invalid token reference: ${value}`);
 if(seen.includes(value))throw Error(`Cyclic token reference: ${value}`);
 const source=match[1]==='primitive'?tokens.primitive:tokens.semantic[theme];
 const target=match[2].split('.').reduce((v,k)=>v?.[k],source);
 if(target===undefined)throw Error(`Missing token: ${value}`);
 return resolve(target,theme,[...seen,value]);
}
function cssValue(k,v){if(typeof v!=='number')return v;if(/weight|lineHeight|strokeWidth/.test(k))return v;if(/duration|transition|reducedMotion/.test(k))return `${v}ms`;return `${v}px`;}
function css(){
 const vars=(data,prefix,theme)=>Object.entries(flatten(data)).map(([k,v])=>`  --${prefix}-${kebab(k)}: ${cssValue(k,resolve(v,theme))};`).join('\n');
 const root=vars(tokens.primitive,'p','dark');
 const themes=['dark','light'].map(theme=>`[data-theme="${theme}"] {\n  color-scheme: ${theme};\n${vars(tokens.semantic[theme],'s',theme)}\n${vars(tokens.component,'c',theme)}\n}`).join('\n');
 return `/* Generated from design-tokens.json. Do not edit. */\n:root {\n${root}\n}\n${themes}\n`;
}
module.exports={tokens,flatten,resolve,kebab,css};
