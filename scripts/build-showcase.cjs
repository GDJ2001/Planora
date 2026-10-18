const fs=require('node:fs');
const path=require('node:path');
const {tokens,resolve,css}=require('./token-utils.cjs');
const root=path.resolve(__dirname,'..'),base=path.join(root,'assets/brand');
const read=p=>fs.readFileSync(path.join(base,p),'utf8');
const data=p=>'data:image/svg+xml;base64,'+Buffer.from(read(p)).toString('base64');
const image=(p,alt,cls='')=>`<img src="${data(p)}" alt="${alt}" class="${cls}"/>`;
const icon=name=>read(`ui/planora-${name}.svg`).replace(/role="img" aria-label="[^"]+"/,'aria-hidden="true" focusable="false"');
const resources=[['cpu','CPU','24','%','12 logical processors'],['memory','Memory','12.8','GB','of 32 GB installed'],['gpu','GPU','18','%','3.1 GB dedicated'],['disk','Disk','8','%','42 MB/s transfer'],['network','Network','2.4','Mbps','Receiving · sample']];
const series={cpu:[14,19,16,34,26,25,48,42,20,30,26,44,24,26,32,22,24],memory:[39,39,40,40,39,40,41,40,40,39,40,40,40,41,40,40,40],gpu:[8,14,12,20,16,17,30,25,14,12,18,30,26,14,20,15,18],disk:[2,4,3,45,18,2,2,1,20,5,2,17,11,3,6,4,8],network:[10,24,13,32,18,8,30,22,14,45,29,24,12,29,18,22,24]};
function points(values,w,h){return values.map((v,i)=>`${(i*w/(values.length-1)).toFixed(1)},${(h-v*h/100).toFixed(1)}`).join(' ');}
const cards=resources.map(([key,label,value,unit,detail])=>`<article class="metric" style="--series:var(--s-resource-${key})"><div class="metric-label">${icon(key)}${label}</div><p class="reading">${value}<span>${unit}</span></p><p class="muted small">${detail}</p><svg class="spark" viewBox="0 0 180 52" aria-hidden="true"><polyline points="${points(series[key],180,52)}"/></svg></article>`).join('');
const rows=[['Browser','4280','12.4%','2.10 GB','4.2 MB/s','1.8 Mbps','B'],['Code editor','6352','6.8%','1.42 GB','0.8 MB/s','0.2 Mbps','C'],['Desktop compositor','1824','2.6%','312 MB','0.1 MB/s','0 Mbps','D'],['File explorer','2468','1.2%','186 MB','0.3 MB/s','0 Mbps','F'],['Planora · preview','8192','0.4%','84 MB','0 MB/s','0 Mbps','P']];
const table=rows.map(([name,pid,cpu,memory,disk,network,letter])=>`<tr data-process="${name.toLowerCase()}"><th scope="row"><span class="process-avatar">${letter}</span>${name}</th><td class="mono muted">${pid}</td><td>${cpu}</td><td>${memory}</td><td>${disk}</td><td>${network}</td></tr>`).join('');
const swatches=Object.entries(tokens.primitive.color.brand).map(([name,hex])=>`<div class="swatch"><div style="background:${hex}" aria-hidden="true"></div><strong>${name[0].toUpperCase()+name.slice(1)}</strong><code>${hex}</code></div>`).join('');
const iconNames=fs.readdirSync(path.join(base,'ui')).filter(f=>f.endsWith('.svg')).map(f=>f.replace('planora-','').replace('.svg',''));
const iconGrid=iconNames.map(name=>`<div class="icon-cell">${icon(name)}<span>${name.replaceAll('-',' ')}</span></div>`).join('');
const iconSizes=[16,20,24,32,48,64,128].map(size=>`<div class="size-cell"><img src="data:image/png;base64,${fs.readFileSync(path.join(base,`platforms/windows/planora-${size}.png`)).toString('base64')}" width="${size}" height="${size}" alt="Planora at ${size} pixels"/><code>${size}px</code></div>`).join('');
const semanticNames=['background','surface','text','textSecondary','primary','accent','success','warning','error'];
const semanticRows=semanticNames.map(name=>`<tr><th scope="row">${name.replace(/[A-Z]/g,c=>' '+c.toLowerCase())}</th><td><span class="color-dot" style="background:${resolve(tokens.semantic.dark[name],'dark')}"></span><code>${resolve(tokens.semantic.dark[name],'dark')}</code></td><td><span class="color-dot" style="background:${resolve(tokens.semantic.light[name],'light')}"></span><code>${resolve(tokens.semantic.light[name],'light')}</code></td></tr>`).join('');
const resRows=resources.map(([key,label])=>`<tr><th scope="row">${label}</th><td><code>${resolve(tokens.semantic.dark.resource[key],'dark')}</code></td><td><code>${resolve(tokens.semantic.light.resource[key],'light')}</code></td></tr>`).join('');
const substitutes={TOKENS:css(),HEADER_LOGO:image('logos/planora-wordmark-dark.svg','Planora','wordmark for-dark')+image('logos/planora-wordmark-light.svg','Planora','wordmark for-light'),HERO_MARK:image('logos/planora-mark.svg','Planora blue orbit mark','hero-mark'),METRICS:cards,PROCESS_ROWS:table,BRAND_SWATCHES:swatches,SEMANTIC_ROWS:semanticRows,RESOURCE_ROWS:resRows,ICON_GRID:iconGrid,ICON_SIZES:iconSizes,
LOGO_LIGHT:image('logos/planora-horizontal-light.svg','Planora logo on a light background'),LOGO_DARK:image('logos/planora-horizontal-dark.svg','Planora logo on a dark background'),MONO:image('logos/planora-monochrome-navy.svg','Planora monochrome logo'),STACKED:image('logos/planora-stacked-dark.svg','Planora stacked logo'),MAC_ICON:image('masters/planora-app-icon-macos.svg','Planora macOS icon'),TRAY_LIGHT:image('platforms/tray/planora-tray-light.svg','White tray mark'),TRAY_DARK:image('platforms/tray/planora-tray-dark.svg','Dark tray mark'),CPU_LINE:points(series.cpu,720,180),MEMORY_LINE:points(series.memory,720,180),SEARCH_ICON:icon('search'),CHECK_ICON:icon('check'),WARNING_ICON:icon('warning'),ERROR_ICON:icon('error'),OVERVIEW_ICON:icon('overview')};
let html=fs.readFileSync(path.join(__dirname,'showcase.template.html'),'utf8');
html=html.replace(/\{\{([A-Z_]+)\}\}/g,(_,name)=>{if(!(name in substitutes))throw Error(`Missing template value ${name}`);return substitutes[name];});
fs.writeFileSync(path.join(base,'showcase.html'),html);
fs.writeFileSync(path.join(root,'assets/design-tokens.css'),css());
console.log('Built self-contained showcase and generated CSS tokens.');
