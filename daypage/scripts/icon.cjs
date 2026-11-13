const sharp = require(process.env.DAYPAGE_SHARP_MODULE || 'sharp');
const fs = require('node:fs');
const path = require('node:path');
const svg = Buffer.from('<svg xmlns="http://www.w3.org/2000/svg" width="256" height="256"><rect width="256" height="256" rx="46" fill="#424b54"/><path d="M78 55H178V201H78ZM78 128H178" fill="none" stroke="white" stroke-width="17"/></svg>');
(async () => {
  const folder=path.resolve(__dirname,'../src-tauri/icons');
  for (const [name,size] of [['32x32.png',32],['128x128.png',128],['128x128@2x.png',256]]) {
    await sharp(svg).resize(size,size).png().toFile(path.join(folder,name));
  }
  const images=await Promise.all([16,24,32,48,64,128,256].map(size=>sharp(svg).resize(size,size).png().toBuffer()));
  const sizes=[16,24,32,48,64,128,256];
  const header=Buffer.alloc(6+16*images.length);
  header.writeUInt16LE(1,2);header.writeUInt16LE(images.length,4);
  let offset=header.length;
  images.forEach((data,index)=>{
    const pos=6+16*index;
    header[pos]=sizes[index]===256?0:sizes[index];header[pos+1]=header[pos];
    header.writeUInt16LE(1,pos+4);header.writeUInt16LE(32,pos+6);
    header.writeUInt32LE(data.length,pos+8);header.writeUInt32LE(offset,pos+12);offset+=data.length;
  });
  fs.writeFileSync(path.join(folder,'icon.ico'),Buffer.concat([header,...images]));
})().catch(error=>{console.error(error);process.exitCode=1;});
