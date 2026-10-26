import type {Descriptor} from './gateway';
const uuid = /^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/;
/** Routing hints only. Session authenticates every publication against preserved signed history. */
export function epochRoute(root: Descriptor, objects: Record<string,string>) {
  if (Object.keys(objects).length > 128) throw new Error('Too many project epochs.');
  const pending = Object.entries(objects).map(([object,line]) => {
    const record=JSON.parse(line).r, key=record?.data;
    if (record?.op !== 'project.key' || record.project !== root.project || key?.project !== root.project || !uuid.test(key.epoch) || !uuid.test(record.id) || object !== `keys/${key.epoch}-${record.id}.json`) throw new Error('Invalid project epoch discovery.');
    return key as {epoch:string; parent:string; parent_recipient:string; recipient:string};
  });
  let epoch='root', recipient=root.recipient;
  const history: string[]=[];
  while (pending.length) {
    const children=pending.filter(key=>key.parent===epoch);
    if (children.length !== 1 || children[0].parent_recipient !== recipient) throw new Error('Project epoch lineage is ambiguous.');
    history.push(epoch);
    const key=children[0]; pending.splice(pending.indexOf(key),1); epoch=key.epoch; recipient=key.recipient;
  }
  return {epoch, prefix:epoch==='root' ? '' : `project/epochs/${epoch}/`, history};
}
