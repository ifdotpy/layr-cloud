import { expect, type Page } from '@playwright/test';
import type { Attestation, Change, ChangeDiff, Copy, Diff, FileContent, Json, Line, Machine, MergeStatus, Operation, Overlay, Person, Policy, Project, QueueEntry, Ref, RefChange, State, Store, Tree, You } from '../src/data/source';
import { fixture } from './fixture-data';

export interface PageCase {
  name: string;
  path: string;
  heading?: string;
  check: (page: Page) => Promise<void>;
}

const projects = fixture<Project[]>('projects');
if (!projects.length) throw new Error('Page tests require a project in projects.json.');
const project = projects[0];
const lines = fixture<Line[]>('lines');
const states = fixture<State[]>('log');
const automatic = fixture<State[]>('log-auto');
const operations = fixture<Operation[]>('operations-all', fixture<Operation[]>('operations'));
const refs = fixture<Ref[]>('refs');
const queue = fixture<QueueEntry[]>('queue');
const change = fixture<Change>('change');
const changes = fixture<Change[]>('changes', [change]);
const copies = fixture<Copy[]>('copies', []);
const people = fixture<Person[]>('people', []);
const machines = fixture<Machine[]>('machines', []);
const stores = fixture<Store[]>('stores', []);
const you = fixture<You>('you', { invites: [], disk: [] });
const trees = fixture<Tree[]>('trees', []);
const files = fixture<FileContent[]>('files', []);
const request = fixture<{ attestation_state: string; ref: string; probe_base: string; probe_head: string }>('request');
const selectedState = automatic.find(state => state.id === request.attestation_state)!;
const attestations = fixture<Attestation[]>('attestations-all', fixture<Attestation[]>('attestations'))
  .filter(attestation => attestation.subject.id === selectedState.id);
const selectedVersion = change.versions.at(-1)!;
const mainLine = lines.find(line => line.name === (project.default_line ?? 'main')) ?? lines[0];
const rootTree = trees.find(tree => tree.state === mainLine.head && tree.path === '');
const config = project.config ?? fixture<Record<string, Json>>('config', {});
const capturedConfig = fixture<{ policy?: Policy }>('config', {});
const policy = project.policy ?? fixture<Policy>('policy', capturedConfig.policy ?? { access: {}, grants: {}, deny: {}, protect: {} });
const overlays = project.overlays ?? fixture<Overlay[]>('overlays', []);
const mergeStatuses = fixture<MergeStatus[]>('merge_status_browser', []);
const interdiffs = ['interdiff', 'interdiff-replay'].flatMap(name => {
  const value = fixture<ChangeDiff | null>(name, null);
  return value ? [value] : [];
});
const short = (id: string) => id.replaceAll('-', '').slice(0, 12);
const route = (path: string) => `/project/${encodeURIComponent(project.id)}/${path}`;
const main = (page: Page) => page.locator('main');
const text = async (page: Page, value: string) => { await expect(main(page)).toContainText(value); };
const table = async (page: Page, name: string, count: number) => {
  const target = main(page).getByRole('table', { name, exact: true });
  await expect(target).toBeVisible();
  await expect(target.locator('tbody tr')).toHaveCount(count);
  return target;
};
const section = (page: Page, heading: string) => main(page).locator('section').filter({
  has: page.getByRole('heading', { name: heading, exact: true }),
});

export const pages: PageCase[] = [
  {
    name: 'Projects', path: '/projects',
    check: async page => {
      await expect(main(page).locator('.list > .item')).toHaveCount(projects.length);
      for (const item of projects) await text(page, item.name);
      await text(page, `${lines.length} lines`);
    },
  },
  {
    name: 'People', path: '/people', heading: 'People',
    check: async page => {
      if (people.length) {
        await table(page, 'Organization people', people.length);
        for (const person of people) await text(page, person.login);
      } else await text(page, 'No people are available from the organization records.');
    },
  },
  {
    name: 'Infrastructure', path: '/infrastructure', heading: 'Infrastructure',
    check: async page => {
      if (stores.length) await table(page, 'Project stores', stores.length);
      else await text(page, 'No stores are available from the store records.');
      if (machines.length) await table(page, 'Organization machines', machines.length);
      else await text(page, 'No machines are available from the machine records.');
      await text(page, 'Gateway details are not available from the deployment records.');
    },
  },
  {
    name: 'Organization settings', path: '/org-settings', heading: 'Organization settings',
    check: async page => {
      await text(page, 'AWS account details are not available from the organization records.');
      await expect(main(page).getByRole('button', { name: 'Delete organization', exact: true })).toBeDisabled();
    },
  },
  {
    name: 'You', path: '/you', heading: 'You',
    check: async page => {
      if (you.recipient) await text(page, you.recipient);
      else await text(page, 'No person key is available from your session.');
      if (you.invites.length) await table(page, 'Your invites', you.invites.length);
      else await text(page, 'No invites are available from your session.');
      if (you.disk.length) await table(page, 'Your disk', you.disk.length);
      else await text(page, 'No disk records are available from your session.');
    },
  },
  {
    name: 'Code', path: route('code'),
    check: async page => {
      if (!rootTree) throw new Error(`Missing root tree fixture for ${mainLine.head}`);
      await table(page, 'Files', rootTree.entries.length);
      for (const entry of rootTree.entries) await text(page, entry.name);
      await text(page, `${lines.length} lines`);
      await text(page, `${refs.length} refs`);
      await text(page, `${states.length} states`);
    },
  },
  {
    name: 'Lines', path: route('lines'), heading: 'Lines',
    check: async page => {
      const visible = lines.filter(line => !line.name.startsWith('agent-'));
      const target = await table(page, 'Lines', visible.length);
      for (const line of visible) {
        const row = target.getByRole('row').filter({ has: page.getByRole('link', { name: line.name, exact: true }) });
        const owner = operations.find(operation => operation.machine === line.machine && operation.actor.uid === line.owner && operation.actor.name)?.actor.name;
        const machine = machines.find(machine => machine.id === line.machine);
        await expect(row.getByRole('cell').nth(1)).toContainText(owner ?? `uid ${line.owner}`);
        await expect(row.getByRole('cell').nth(3)).toHaveText(machine?.name ?? `machine ${short(line.machine).slice(0, 7)}`);
      }
      if (!project.disk?.length) {
        await text(page, 'Disk use is not recorded for this store.');
        await expect(main(page).locator('.card').filter({ hasText: 'Disk use' })).toHaveCount(0);
      }
    },
  },
  {
    name: 'States', path: route('states'), heading: 'States',
    check: async page => {
      await expect(main(page).locator('.list > .item')).toHaveCount(states.length);
      await text(page, `${states.length} states`);
      await text(page, states[0].message);
      await main(page).getByRole('checkbox', { name: 'Show automatic states' }).check();
      await expect(main(page).locator('.list > .item')).toHaveCount(automatic.length);
      await text(page, `${automatic.length} states`);
      await main(page).getByRole('checkbox', { name: 'Show automatic states' }).uncheck();
      await expect(main(page).locator('.list > .item')).toHaveCount(states.length);
    },
  },
  {
    name: 'State', path: route(`state/${selectedState.id}`), heading: selectedState.message,
    check: async page => {
      await text(page, selectedState.id);
      await text(page, selectedState.author.name);
      await expect(section(page, 'Attestations').locator('tbody tr')).toHaveCount(attestations.length);
      for (const attestation of attestations) await text(page, attestation.kind);
    },
  },
];

function expectedChanges(status: 'open' | 'merged' | 'closed') {
  return changes.filter(item => {
    const entries = queue.filter(entry => entry.change_id === item.change_id || item.versions.some(version => version.state.id === entry.state));
    const merged = entries.some(entry => entry.status === 'done') || mergeStatuses.some(evaluation =>
      evaluation.state === item.versions.at(-1)!.state.id && evaluation.target === (project.default_line ?? 'main') && evaluation.already_in_target);
    const queued = entries.some(entry => ['pending', 'queued', 'running'].includes(entry.status));
    return status === 'merged' ? merged : status === 'open' ? !merged && !queued : false;
  });
}

export const openChangeCount = expectedChanges('open').length;

for (const status of ['open', 'merged', 'closed'] as const) {
  const expected = expectedChanges(status);
  pages.push({
    name: `Changes ${status}`, path: route(`changes${status === 'open' ? '' : '/' + status}`), heading: 'Changes',
    check: async page => {
      await text(page, `${expected.length} changes`);
      await expect(main(page).locator('.list > .item')).toHaveCount(expected.length);
      for (const item of expected) await text(page, item.versions.at(-1)!.state.message);
      if (!expected.length) await text(page, `No ${status} changes recorded.`);
    },
  });
}

pages.push({
  name: 'Changes queue', path: route('changes/queue'), heading: 'Queue for main',
  check: async page => {
    await table(page, 'Queue', queue.length);
    for (const entry of queue) {
      await text(page, short(entry.change_id ?? entry.state));
      if (entry.reason) await text(page, entry.reason);
    }
  },
});

for (const tab of ['overview', 'files', 'behavior', 'steps']) {
  pages.push({
    name: `Change ${tab}`, path: route(`change/${change.change_id}/${tab}`), heading: selectedVersion.state.message,
    check: async page => {
      await expect(main(page).getByRole('combobox', { name: 'Version', exact: true }).locator('option')).toHaveCount(change.versions.length);
      await text(page, `${change.versions.length} saved versions`);
      if (tab === 'overview') {
        await expect(section(page, 'Versions').locator('li')).toHaveCount(change.versions.length);
        await table(page, 'Change checks', selectedVersion.attestations.filter(item => item.kind === 'check').length);
        await expect(section(page, 'Attestations').locator('tbody tr')).toHaveCount(selectedVersion.attestations.length);
        const evaluation = mergeStatuses.find(value => value.state === selectedVersion.state.id && value.target === (project.default_line ?? 'main'));
        if (evaluation) await text(page, `${evaluation.approvals.filter(item => item.met === true).length} of ${evaluation.approvals_needed} approvals`);
      } else if (tab === 'files') {
        const previous = change.versions.at(-2) ?? change.versions[0];
        const head = selectedVersion.state.id;
        const reviewed = [...change.versions].reverse().find(version => you.login && version.attestations.some(
          attestation => attestation.kind === 'approval' && attestation.signer === `person:${you.login}`));
        const inspectDiff = async (base: string, missing: string, fromVersion?: string, toVersion = selectedVersion.version) => {
          const native = interdiffs.find(value => value.change_id === change.change_id && value.from === fromVersion && value.to === toVersion);
          if (native) {
            await expect(main(page).locator('.compare-bar')).toContainText(`${native.files.length} files`);
            await expect(main(page).locator('.files-grid .file-link')).toHaveCount(native.files.length);
            if (!native.files.length) await text(page, 'No code changes between these versions.');
            else {
              const file = native.files[0];
              await main(page).getByRole('button', { name: file.path, exact: true }).click();
              const diff = main(page).getByRole('table', { name: `Diff of ${file.path}`, exact: true });
              for (const line of (file.interdiff ?? '').split('\n').filter(line => line.trim()).slice(0, 3)) {
                await expect(diff).toContainText(line);
              }
              await main(page).locator('.compare-bar').getByRole('button', { name: 'Split', exact: true }).click();
              await expect(diff.locator('tr:not(.h)').first().locator('td')).toHaveCount(2);
              await main(page).locator('.compare-bar').getByRole('button', { name: 'Unified', exact: true }).click();
              await expect(diff.locator('tr:not(.h)').first().locator('td')).toHaveCount(1);
            }
            return true;
          }
          if (!trees.some(tree => tree.state === base && tree.path === '')) {
            await text(page, missing);
            return false;
          }
          const paths = new Set(files.filter(file => file.state === base || file.state === head).map(file => file.path));
          const changed = [...paths].filter(path => {
            const before = files.find(file => file.state === base && file.path === path);
            const after = files.find(file => file.state === head && file.path === path);
            return before?.text !== after?.text || before?.base64 !== after?.base64 || before?.size !== after?.size;
          });
          const captured = fixture<Diff | null>('diff', null);
          const exact = captured?.base === base && captured.head === head ? captured : undefined;
          const expectedPaths = exact ? exact.files.map(file => file.path) : changed;
          await expect(main(page).locator('.compare-bar')).toContainText(`${expectedPaths.length} files`);
          await expect(main(page).locator('.files-grid .file-link')).toHaveCount(expectedPaths.length);
          for (const path of expectedPaths) await text(page, path);
          if (expectedPaths.length) {
            const path = expectedPaths[0];
            const button = main(page).getByRole('button', { name: path, exact: true });
            await button.click();
            await expect(button).toHaveAttribute('aria-current', 'true');
            const content = exact?.files.find(file => file.path === path)?.after
              ?? files.find(file => file.state === head && file.path === path)?.text
              ?? exact?.files.find(file => file.path === path)?.before
              ?? files.find(file => file.state === base && file.path === path)?.text;
            const firstLine = content?.split('\n').find(line => line.trim());
            if (firstLine) await expect(main(page).getByRole('table', { name: `Diff of ${path}`, exact: true })).toContainText(firstLine);
          }
          return true;
        };
        const compare = main(page).locator('.compare-bar > .menu-wrap > button');
        const choices = main(page).locator('.compare-bar .menu');
        const openCompare = async () => {
          await compare.click();
          await expect(compare).toHaveAttribute('aria-expanded', 'true');
        };
        const closeCompare = async () => {
          await page.keyboard.press('Escape');
          await expect(compare).toHaveAttribute('aria-expanded', 'false');
        };
        await expect(compare).toContainText('Changes since your review');
        await inspectDiff(reviewed?.state.id ?? selectedVersion.state.parents[0], reviewed
          ? `Version ${reviewed.version} is not in this store yet.` : 'The base is not in this store yet.', reviewed?.version);
        await openCompare();
        await choices.getByRole('button', { name: /^Only the latest update/ }).click();
        await expect(compare).toHaveAttribute('aria-expanded', 'false');
        await openCompare();
        await expect(main(page).getByRole('combobox', { name: 'From version', exact: true })).toHaveValue(previous.version);
        await closeCompare();
        if (!await inspectDiff(previous.state.id, `Version ${previous.version} is not in this store yet.`, previous.version)) {
          await main(page).getByRole('button', { name: `Compare ${selectedVersion.version} against its base`, exact: true }).click();
          await openCompare();
          await expect(main(page).getByRole('combobox', { name: 'From version', exact: true })).toHaveValue('base');
          await closeCompare();
          await inspectDiff(selectedVersion.state.parents[0], 'The base is not in this store yet.');
        }
        for (const native of interdiffs.filter(value => value.change_id === change.change_id)) {
          await openCompare();
          await main(page).getByRole('combobox', { name: 'From version', exact: true }).selectOption(native.from);
          await main(page).getByRole('combobox', { name: 'To version', exact: true }).selectOption(native.to);
          await closeCompare();
          const version = change.versions.find(value => value.version === native.from)!;
          await inspectDiff(version.state.id, `Version ${native.from} is not in this store yet.`, native.from, native.to);
        }
        await openCompare();
        await choices.getByRole('button', { name: /^Since your review/ }).click();
        await expect(compare).toHaveAttribute('aria-expanded', 'false');
        await inspectDiff(reviewed?.state.id ?? selectedVersion.state.parents[0], reviewed
          ? `Version ${reviewed.version} is not in this store yet.` : 'The base is not in this store yet.', reviewed?.version);
      } else if (tab === 'behavior') {
        for (const probe of change.behavior.probes ?? []) {
          await text(page, probe.probe);
          await text(page, probe.status);
        }
        if (!change.behavior.probes?.length) await text(page, 'No probe results recorded.');
      } else {
        const steps = operations.flatMap(operation => operation.states ?? []).filter(state =>
          state.kind === 'auto' && state.author.session && (state.change_id === change.change_id || state.line === selectedVersion.state.line));
        if (steps.length) {
          await text(page, `${steps.length} steps.`);
          await expect(main(page).locator('.tl > li')).toHaveCount(steps.length);
        } else await text(page, 'No agent steps recorded for this change.');
      }
    },
  });
}

const checkDefinitions = Object.keys(config).filter(key => key.startsWith('check.'));
const storedChecks = operations.flatMap(operation => {
  const data = operation.data;
  if (!data || typeof data !== 'object' || Array.isArray(data)) return [];
  const check = data.check;
  return check && typeof check === 'object' && !Array.isArray(check) && !check.probe && typeof check.exit === 'number' ? [check] : [];
});

pages.push(
  {
    name: 'Checks', path: route('checks'), heading: 'Checks',
    check: async page => {
      await table(page, 'Check results', project.checks?.length ?? checkDefinitions.length + storedChecks.length);
      if (project.checks) for (const check of project.checks) await text(page, check.name);
      else for (const definition of checkDefinitions) await text(page, definition.slice('check.'.length));
    },
  },
  {
    name: 'Checks behavior', path: route('checks/behavior'), heading: 'Behavior',
    check: async page => {
      const probeDefinitions = project.probes?.length ?? Object.keys(config).filter(key => key.startsWith('probe.')).length;
      await table(page, 'Probe definitions', probeDefinitions);
      await expect(main(page).getByRole('combobox', { name: 'Base', exact: true }).locator('option')).toHaveCount(states.length);
      await main(page).getByRole('combobox', { name: 'Base', exact: true }).selectOption(request.probe_base);
      await main(page).getByRole('combobox', { name: 'Head', exact: true }).selectOption(request.probe_head);
      await main(page).getByRole('button', { name: 'Compare', exact: true }).click();
      const behavior = fixture<{ probes?: { probe: string; status: string }[] }>('probe-diff');
      for (const probe of behavior.probes ?? []) {
        await text(page, probe.probe);
        await text(page, probe.status);
      }
    },
  },
  {
    name: 'Deploy environments', path: route('deploy/environments'), heading: 'Environments',
    check: async page => {
      const envs = refs.filter(ref => ref.name.startsWith('env/'));
      await table(page, 'Environments', envs.length);
      await expect(section(page, 'Other refs').locator('tbody tr')).toHaveCount(refs.length - envs.length);
      for (const ref of refs) await text(page, ref.name);
    },
  },
  {
    name: 'Deploy environment', path: route(`deploy/environment/${encodeURIComponent(request.ref)}`), heading: request.ref,
    check: async page => {
      const history = fixture<RefChange[]>('ref-log').filter(ref => ref.name === request.ref);
      await table(page, 'Ref history', history.length);
      for (const ref of history) if (ref.after) await text(page, short(ref.after.target));
    },
  },
  {
    name: 'Deploy images', path: route('deploy/images'), heading: 'Images',
    check: async page => {
      await expect(section(page, 'Images').locator('tbody tr')).toHaveCount(project.images?.length ?? 0);
      for (const image of project.images ?? []) {
        await text(page, image.name);
        await text(page, image.tag);
      }
      if (!project.images?.length) await text(page, 'No image records available. The image service supplies digests and tags.');
      await text(page, 'No chart records available. The image service supplies chart versions.');
    },
  },
  {
    name: 'Activity', path: route('activity'), heading: 'Activity',
    check: async page => {
      await table(page, 'Operations', operations.length);
      if (operations.length) await text(page, operations[0].actor.session ?? operations[0].actor.name);
      await expect(main(page).getByRole('combobox', { name: 'Person filter' }).locator('option')).toHaveCount(new Set(operations.map(item => item.actor.name)).size + 1);
      await expect(main(page).getByRole('combobox', { name: 'Operation filter' }).locator('option')).toHaveCount(new Set(operations.map(item => item.op)).size + 1);
    },
  },
);

for (const [name, path, heading, empty] of [
  ['Access', 'access', 'Access', ''],
  ['Protection', 'protection', 'Protection', ''],
  ['Private files', 'private-files', 'Private files', 'No detector paths are available from the project configuration.'],
  ['Files', 'files', 'Merge rules', 'No merge rules are available from the project configuration.'],
  ['Copies', 'copies', 'Copies', ''],
]) {
  pages.push({
    name: `Settings ${name}`, path: route(`settings/${path}`),
    check: async page => {
      await expect(main(page).getByRole('heading', { name: heading, exact: true })).toBeVisible();
      await expect(main(page).getByRole('navigation', { name: 'Settings', exact: true }).getByRole('link', { name, exact: true })).toHaveAttribute('aria-current', 'page');
      if (empty) await text(page, empty);
      if (path === 'access') {
        const access = Object.entries(policy.access).filter(([, role]) => role !== 'none');
        const grants = Object.entries(policy.grants).filter(([, [role]]) => role !== 'none');
        if (access.length + grants.length) {
          await table(page, 'Project access', access.length + grants.length);
          for (const [principal] of [...access, ...grants]) await text(page, principal);
        } else await text(page, 'No project roles are available from the access policy.');
      }
      if (path === 'protection') {
        const rules = Object.keys(policy.protect);
        if (rules.length) {
          await table(page, 'Protection rules', rules.length);
          for (const name of rules) await text(page, name);
        } else await text(page, 'No protection rules are available from the project policy.');
      }
      if (path === 'files' && !project.disk?.length) await text(page, 'Disk use is not recorded for this store.');
      if (path === 'copies') {
        if (copies.length) {
          await table(page, 'Project copies', copies.length);
          for (const copy of copies) await text(page, copy.name);
        }
        else await text(page, 'No copies are recorded.');
        const ids = new Set([...lines.map(line => line.machine), ...copies.flatMap(copy => copy.machines ?? [])]);
        await table(page, 'Machines with this project', ids.size);
      }
    },
  });
}

const codeFile = files.find(file => file.state === mainLine.head && file.text !== undefined);
if (codeFile) pages.push({
  name: 'Code file', path: route(`code?path=${encodeURIComponent(codeFile.path)}`),
  check: async page => {
    await expect(main(page).locator('.frame')).toContainText(codeFile.path);
    await expect(main(page).locator('pre.code')).toContainText(codeFile.text!.trim());
  },
});

const nestedTree = trees.find(tree => tree.state === mainLine.head && tree.path !== '');
if (nestedTree) pages.push({
  name: 'Code folder', path: route(`code?path=${encodeURIComponent(nestedTree.path)}`),
  check: async page => {
    await table(page, 'Files', nestedTree.entries.length + 1);
    for (const entry of nestedTree.entries) await text(page, entry.name);
    const readme = files.find(file => file.state === nestedTree.state && file.path === `${nestedTree.path}/README.md`);
    if (readme?.text) {
      const heading = readme.text.split('\n').find(line => line.startsWith('# '));
      await expect(main(page).locator('.md')).toBeVisible();
      if (heading) await expect(main(page).locator('.md')).toContainText(heading.slice(2));
    } else await text(page, 'No README.md in this folder.');
  },
});

pages.push({
  name: 'Overlays', path: route('overlays'), heading: 'Overlays',
  check: async page => {
    if (overlays.length) {
      await table(page, 'Project overlays', overlays.length);
      for (const overlay of overlays) await text(page, overlay.name);
    } else await text(page, 'No overlays are recorded.');
  },
});

const overlay = overlays[0];
if (overlay) pages.push({
  name: 'Overlay', path: route(`overlay/${encodeURIComponent(overlay.id)}`), heading: overlay.name,
  check: async page => {
    await table(page, 'Overlay readers', overlay.readers.length);
    for (const reader of overlay.readers) await text(page, reader);
    if (overlay.readable) {
      const paths = overlay.paths ?? overlay.files?.map(file => file.path);
      if (paths?.length) {
        await table(page, 'Overlay files', paths.length);
        for (const path of paths) await text(page, path);
      }
      await text(page, `${overlay.readers.length} reader${overlay.readers.length === 1 ? '' : 's'}`);
      await text(page, `${overlay.managers.length} key manager${overlay.managers.length === 1 ? '' : 's'}`);
      if (overlay.history?.length) await table(page, 'Signed overlay operations', overlay.history.length);
    } else await text(page, 'Files are hidden. You cannot read this overlay.');
  },
});
