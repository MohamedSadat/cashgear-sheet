// `/embed`: Cash Gear Workspace's read-only frame for one range or chart of a workbook (Track S3
// to S5, Workspace's `sheet-embed.1`). A page of its own: it never starts the editor, registers
// no service worker and keeps nothing on the device.
import { startEmbed } from '@cgs/ui/embed';

startEmbed(document.getElementById('root')!);
