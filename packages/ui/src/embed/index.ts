// `/embed`, Cash Gear Workspace's read-only frame (Track S3 to S5): its own page of the web build,
// which never starts the editor.
import '@cashgear/ui/styles.css';
import './embed.css';

export { startEmbed } from './controller';
export { allowedParents, LIVE_PARENTS, LOCAL_PARENTS } from './protocol';
export { CONTRACT, CONTRACTS } from './contract';
