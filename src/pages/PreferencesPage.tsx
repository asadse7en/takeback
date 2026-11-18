import { useEffect, useMemo, useState, type ReactNode } from 'react';
import { useNavigate } from 'react-router-dom';
import {
  Archive, CalendarDays, Check, Copy, FileText, Folder, FolderOpen,
  Grid2X2, Heart, Image, MapPin, Pencil, Settings2, Star, X,
} from 'lucide-react';
import { open } from '@tauri-apps/plugin-dialog';
import { Button, PageFrame, Panel, SegmentedControl, Toggle } from '@/components/Ui';
import { callCommand, type PreviewNode, type PreviewResult } from '@/lib/api';
import { useRunStore } from '@/store/run';
import { useSettingsStore } from '@/store/settings';
import type { Settings } from '@/types/app';

const folderOptions = ['Off', 'Move', 'Link', 'Copy'] as const;
type FolderLabel = typeof folderOptions[number];
type PreviewGuide = 'empty' | 'vertical' | 'tee' | 'last';
type PreviewLine = Omit<PreviewNode, 'children'> & { children: undefined; guides: PreviewGuide[] };

const folderLabels: Record<Settings['albumMode'], FolderLabel> = {
  off: 'Off', move: 'Move', link: 'Link', copy: 'Copy',
};
const folderModes: Record<FolderLabel, Settings['albumMode']> = {
  Off: 'off', Move: 'move', Link: 'link', Copy: 'copy',
};

function flattenTree(nodes: PreviewNode[]): PreviewLine[] {
  const compact = (node: PreviewNode): PreviewNode => {
    let name = node.name;
    let current = node;
    while (current.folder && current.children.length === 1 && current.children[0].folder) {
      current = current.children[0];
      name = `${name} › ${current.name}`;
    }
    return { ...current, name };
  };
  const flatten = (items: PreviewNode[], depth: number, ancestorsContinue: boolean[]): PreviewLine[] => {
    const sorted = [...items].sort((a, b) => Number(b.folder) - Number(a.folder) || a.name.localeCompare(b.name));
    return sorted.flatMap((rawNode, index): PreviewLine[] => {
      const isLast = index === sorted.length - 1;
      const node = compact(rawNode);
      const guides: PreviewGuide[] = [
        ...ancestorsContinue.map((continues): PreviewGuide => continues ? 'vertical' : 'empty'),
      ];
      if (depth > 0) guides.push(isLast ? 'last' : 'tee');
      const childAncestors = depth === 0 ? ancestorsContinue : [...ancestorsContinue, !isLast];
      return [
        { name: node.name, folder: node.folder, tag: node.tag, children: undefined, guides },
        ...flatten(node.children, depth + 1, childAncestors),
      ];
    });
  };
  return flatten(nodes, 0, []);
}

function folderSize(bytes: number) {
  return `${(bytes / 1_000_000_000).toFixed(1)} GB`;
}

function toLocalLabel(mode: Settings['timeZone']) {
  return ({ local: "Photo's local time", computer: 'Computer time', utc: 'UTC' })[mode];
}

export default function PreferencesPage() {
  const navigate = useNavigate();
  const settings = useSettingsStore((state) => state.settings);
  const updateSettings = useSettingsStore((state) => state.updateSettings);
  const replaceSettings = useSettingsStore((state) => state.replaceSettings);
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const [confirmDeleteOpen, setConfirmDeleteOpen] = useState(false);
  const [deleteConfirmation, setDeleteConfirmation] = useState('');
  const [activeTab, setActiveTab] = useState('Folders');
  const [preview, setPreview] = useState<PreviewResult>({
    files: [], tree: [], warnings: [], requiredBytes: 0, freeBytes: 0,
  });
  const [error, setError] = useState('');
  const previewTree = useMemo(() => flattenTree(preview.tree), [preview.tree]);
  const outputName = preview.tree[0]?.name ?? 'Photo Library';
  const freeAfter = Math.max(0, preview.freeBytes - preview.requiredBytes);

  useEffect(() => {
    let active = true;
    callCommand('load_settings', {}).then((loaded) => {
      if (active) replaceSettings(loaded);
    }).catch((reason: unknown) => {
      if (active) setError(reason instanceof Error ? reason.message : String(reason));
    });
    return () => { active = false; };
  }, [replaceSettings]);

  useEffect(() => {
    let active = true;
    const timer = window.setTimeout(() => {
      callCommand('get_preview', { settings }).then((result) => {
        if (active) setPreview(result);
      }).catch((reason: unknown) => {
        if (active) setError(reason instanceof Error ? reason.message : String(reason));
      });
    }, 120);
    return () => {
      active = false;
      window.clearTimeout(timer);
    };
  }, [settings]);

  const updateFolder = (key: 'albumMode' | 'favoritesMode' | 'editedMode', label: string) => {
    if (label in folderModes) updateSettings({ [key]: folderModes[label as FolderLabel] });
  };
  const browseOutput = async () => {
    const selected = await open({ directory: true, multiple: false });
    if (typeof selected === 'string') updateSettings({ outputFolder: selected });
  };
  const start = async () => {
    setError('');
    try {
      await callCommand('save_settings', { settings });
      useRunStore.getState().reset();
      navigate('/progress');
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  };

  return (
    <>
      <PageFrame
        className="preferences-page-frame"
        title="Preferences"
        subtitle="Choose how your library is organized."
        footer={
          <>
            <label className="preview-only">
              <input type="checkbox" checked={settings.dryRun} onChange={(event) => updateSettings({ dryRun: event.target.checked })} />
              Preview only
            </label>
            <div className="footer-buttons">
              <Button onClick={() => navigate('/add')}>Back</Button>
              <Button variant="accent" onClick={() => void start()}>Start</Button>
            </div>
          </>
        }
      >
        <div className="preferences-layout">
          <div className="preferences-main">
            <Panel className="output-folder">
              <strong>Output folder</strong>
              <div className="output-path-row">
                <div className="output-path"><Folder size={17} aria-hidden="true" />{settings.outputFolder ?? outputName}</div>
                <Button onClick={() => void browseOutput()}>Browse</Button>
              </div>
              <div className="disk-meter" aria-label={`Required ${folderSize(preview.requiredBytes)}, free ${folderSize(preview.freeBytes)}`}>
                <span /><span /><span />
              </div>
              <div className="disk-legend">
                <span>Disk space</span>
                <span><i className="swatch swatch--need" />{folderSize(preview.requiredBytes)} this export</span>
                <span><i className="swatch swatch--free" />{folderSize(freeAfter)} free after</span>
              </div>
            </Panel>

            <Panel className="preference-list">
              <PreferenceRow icon={<FolderOpen />} title="Albums" detail="From the indexed Takeout files">
                <SegmentedControl label="Album handling" options={folderOptions} value={folderLabels[settings.albumMode]} onChange={(value) => updateFolder('albumMode', value)} />
              </PreferenceRow>
              <PreferenceRow icon={<CalendarDays />} title="Date folders">
                <SegmentedControl
                  label="Date folders"
                  options={['Off', 'Year', 'Year › Month']}
                  value={settings.dateFolders === 'yearMonth' ? 'Year › Month' : settings.dateFolders === 'year' ? 'Year' : 'Off'}
                  onChange={(value) => updateSettings({ dateFolders: value === 'Year › Month' ? 'yearMonth' : value.toLowerCase() as Settings['dateFolders'] })}
                />
              </PreferenceRow>
              <PreferenceRow icon={<Star />} title="Favorites" detail="Starred photos from Takeout">
                <SegmentedControl label="Favorite handling" options={folderOptions} value={folderLabels[settings.favoritesMode]} onChange={(value) => updateFolder('favoritesMode', value)} />
              </PreferenceRow>
              <PreferenceRow icon={<Pencil />} title="Edited photos" detail="Choose which versions to keep">
                <SegmentedControl
                  label="Edited photos"
                  options={['Keep both', 'Originals only', 'Edited only']}
                  value={settings.editedVersions === 'both' ? 'Keep both' : settings.editedVersions === 'originals' ? 'Originals only' : 'Edited only'}
                  onChange={(value) => updateSettings({ editedVersions: value === 'Keep both' ? 'both' : value === 'Originals only' ? 'originals' : 'edited' })}
                />
              </PreferenceRow>
              <PreferenceRow icon={<Image />} title="Live photos" detail="Set the output pair style in Advanced options">
                <span className="preference-control">
                  {settings.mergeLive ? <span className="tag">{settings.liveStyle}</span> : null}
                  <Toggle label="Keep live photos together" checked={settings.mergeLive} onChange={(mergeLive) => updateSettings({ mergeLive })} />
                </span>
              </PreferenceRow>
              <PreferenceRow icon={<Archive />} title="Archive" detail="Organize archived photos">
                <Toggle label="Organize archived photos" checked={settings.archiveFolder} onChange={(archiveFolder) => updateSettings({ archiveFolder })} />
              </PreferenceRow>
              <PreferenceRow icon={<Settings2 />} title="Advanced options" detail="Names, dates, and metadata">
                <Button onClick={() => setAdvancedOpen(true)}>Open</Button>
              </PreferenceRow>
            </Panel>
          </div>

          <Panel className="output-preview">
            <strong>Output preview</strong>
            <div className="preview-tree" tabIndex={0} aria-label="Output preview, scrollable">
              {previewTree.map((item, index) => (
                <div className="tree-line" key={`${item.name}-${index}`}>
                  <span className="tree-guides" aria-hidden="true">
                    {item.guides.map((guide, guideIndex) => (
                      <span className={`tree-guide tree-guide--${guide}`} key={guideIndex} />
                    ))}
                  </span>
                  {item.folder ? <Folder size={16} aria-hidden="true" /> : <span className="tree-file-icon" />}
                  <span className={item.folder ? 'tree-folder' : 'tree-file'}>{item.name}</span>
                  {item.tag ? <span className="tag">{item.tag}</span> : null}
                </div>
              ))}
              {!previewTree.length ? <p className="empty-state">Add and scan Takeout files to see a preview.</p> : null}
            </div>
            {preview.warnings.map((warning) => <p role="status" key={warning} className="form-error">{warning}</p>)}
          </Panel>
        </div>
        {error ? <p role="alert" className="form-error">{error}</p> : null}
      </PageFrame>

      {advancedOpen ? (
        <div className="modal-backdrop" onMouseDown={(event) => { if (event.target === event.currentTarget) setAdvancedOpen(false); }}>
          <section className="advanced-dialog" role="dialog" aria-modal="true" aria-labelledby="advanced-title">
            <header className="advanced-dialog__header">
              <h2 id="advanced-title">Advanced options</h2>
              <button type="button" className="icon-button" aria-label="Close advanced options" onClick={() => setAdvancedOpen(false)}><X size={16} /></button>
            </header>
            <nav className="advanced-tabs" aria-label="Advanced option categories">
              {['Folders', 'Names and duplicates', 'Dates and metadata', 'More'].map((tab) => (
                <button key={tab} type="button" className={tab === activeTab ? 'is-selected' : ''} onClick={() => setActiveTab(tab)}>{tab}</button>
              ))}
            </nav>
            <div className="advanced-options">
              {activeTab === 'Folders' ? (
                <>
                  <AdvancedRow icon={<Folder />} title="Where albums go" options={['Top level', 'Inside year folders']} value={settings.albumLoc === 'top' ? 'Top level' : 'Inside year folders'} onChange={(value) => updateSettings({ albumLoc: value === 'Top level' ? 'top' : 'year' })} />
                  <AdvancedRow icon={<Copy />} title="Photo in several albums" options={['First album only', 'Keep in all']} value={settings.multiAlbum === 'first' ? 'First album only' : 'Keep in all'} onChange={(value) => updateSettings({ multiAlbum: value === 'First album only' ? 'first' : 'all' })} />
                  <AdvancedRow icon={<Pencil />} title="Edited photos folder" detail="Collect edited versions in one folder" options={folderOptions} value={folderLabels[settings.editedMode]} onChange={(value) => updateFolder('editedMode', value)} />
                  <AdvancedRow icon={<Archive />} title="Trashed items" options={['Skip', 'Include', 'Own folder']} value={settings.trashed === 'ownFolder' ? 'Own folder' : settings.trashed === 'include' ? 'Include' : 'Skip'} onChange={(value) => updateSettings({ trashed: value === 'Own folder' ? 'ownFolder' : value.toLowerCase() as Settings['trashed'] })} />
                  <PreferenceRow icon={<Grid2X2 />} title="Sort into folders by type" detail="Inside every album and date folder">
                    <div className="checkbox-options">
                      <label><input type="checkbox" checked={settings.splitByType.photos} onChange={(event) => updateSettings({ splitByType: { ...settings.splitByType, photos: event.target.checked } })} /> Photos</label>
                      <label><input type="checkbox" checked={settings.splitByType.videos} onChange={(event) => updateSettings({ splitByType: { ...settings.splitByType, videos: event.target.checked } })} /> Videos</label>
                      <label><input type="checkbox" checked={settings.splitByType.live} onChange={(event) => updateSettings({ splitByType: { ...settings.splitByType, live: event.target.checked } })} /> Live</label>
                    </div>
                  </PreferenceRow>
                </>
              ) : null}
              {confirmDeleteOpen ? (
                <div className="modal-backdrop" onMouseDown={(event) => { if (event.target === event.currentTarget) setConfirmDeleteOpen(false); }}>
                  <section className="advanced-dialog" role="dialog" aria-modal="true" aria-labelledby="delete-confirm-title">
                    <header className="advanced-dialog__header"><h2 id="delete-confirm-title">Send processed archives to the Recycle Bin?</h2></header>
                    <p className="delete-confirm-copy">Eligible Takeout archives are sent to your system's Recycle Bin or Trash only after the entire run finishes without failures. You can restore them there. To run Takeback again, you would need to download your Takeout again. Your Google Photos library is not affected.</p>
                    <label className="delete-confirm-field">
                      Type DELETE to confirm
                      <input
                        autoComplete="off"
                        value={deleteConfirmation}
                        onChange={(event) => setDeleteConfirmation(event.target.value)}
                      />
                    </label>
                    <footer className="advanced-dialog__footer">
                      <Button onClick={() => {
                        setConfirmDeleteOpen(false);
                        setDeleteConfirmation('');
                      }}>Cancel</Button>
                      <Button variant="accent" onClick={() => {
                        updateSettings({ deleteArchivesAfter: true });
                        setConfirmDeleteOpen(false);
                        setDeleteConfirmation('');
                      }} disabled={deleteConfirmation !== 'DELETE'}>Send archives to Recycle Bin</Button>
                    </footer>
                  </section>
                </div>
              ) : null}
              {activeTab === 'Names and duplicates' ? (
                <>
                  <AdvancedRow icon={<FileText />} title="File names" options={['Keep', 'Date and time', 'Date and original', 'Custom']} value={settings.fileNames === 'dateTime' ? 'Date and time' : settings.fileNames === 'dateOriginal' ? 'Date and original' : settings.fileNames === 'custom' ? 'Custom' : 'Keep'} onChange={(value) => updateSettings({ fileNames: value === 'Date and time' ? 'dateTime' : value === 'Date and original' ? 'dateOriginal' : value === 'Custom' ? 'custom' : 'keep' })} />
                  {settings.fileNames === 'custom' ? <label className="custom-pattern">Naming pattern<input value={settings.customNamePattern} onChange={(event) => updateSettings({ customNamePattern: event.target.value })} /></label> : null}
                  <AdvancedRow icon={<Copy />} title="If two files have the same name" options={['Add a number', 'Use date and time', 'Skip']} value={settings.sameName === 'dateTime' ? 'Use date and time' : settings.sameName === 'skip' ? 'Skip' : 'Add a number'} onChange={(value) => updateSettings({ sameName: value === 'Use date and time' ? 'dateTime' : value === 'Skip' ? 'skip' : 'number' })} />
                  <AdvancedToggle icon={<Check />} title="Ignore exact duplicates" checked={settings.ignoreDuplicates} onChange={(ignoreDuplicates) => updateSettings({ ignoreDuplicates })} />
                  <AdvancedRow icon={<Folder />} title="Files without JSON" options={['Leave in place', 'Put in Unsorted']} value={settings.noJson === 'leave' ? 'Leave in place' : 'Put in Unsorted'} onChange={(value) => updateSettings({ noJson: value === 'Leave in place' ? 'leave' : 'unsorted' })} />
                </>
              ) : null}
              {activeTab === 'Dates and metadata' ? (
                <>
                  <AdvancedRow icon={<CalendarDays />} title="Time zone" options={["Photo's local time", 'Computer time', 'UTC']} value={toLocalLabel(settings.timeZone)} onChange={(value) => updateSettings({ timeZone: value === 'Computer time' ? 'computer' : value === 'UTC' ? 'utc' : 'local' })} />
                  <AdvancedRow icon={<CalendarDays />} title="If a photo has no date" options={['Use file name', 'Use file date', 'Leave blank']} value={settings.noDate === 'fileName' ? 'Use file name' : settings.noDate === 'fileDate' ? 'Use file date' : 'Leave blank'} onChange={(value) => updateSettings({ noDate: value === 'Use file date' ? 'fileDate' : value === 'Leave blank' ? 'blank' : 'fileName' })} />
                  <AdvancedToggle icon={<MapPin />} title="Write location" checked={settings.writeGps} onChange={(writeGps) => updateSettings({ writeGps })} />
                  <AdvancedToggle icon={<Pencil />} title="Write descriptions and people tags" checked={settings.writeDescriptions} onChange={(writeDescriptions) => updateSettings({ writeDescriptions })} />
                  <AdvancedToggle icon={<Heart />} title="Rate favorites 5 stars" checked={settings.rateFavorites} onChange={(rateFavorites) => updateSettings({ rateFavorites })} />
                </>
              ) : null}
              {activeTab === 'More' ? (
                <>
                  <AdvancedRow icon={<Image />} title="Live photo style" options={['iPhone', 'Android', 'Separate files']} value={settings.liveStyle === 'iphonePair' ? 'iPhone' : settings.liveStyle === 'androidMotionPhoto' ? 'Android' : 'Separate files'} onChange={(value) => updateSettings({ liveStyle: value === 'iPhone' ? 'iphonePair' : value === 'Android' ? 'androidMotionPhoto' : 'separate' })} />
                  <AdvancedToggle icon={<Archive />} title="Send processed archives to Recycle Bin" checked={settings.deleteArchivesAfter} onChange={(enabled) => {
                    if (enabled) {
                      setDeleteConfirmation('');
                      setConfirmDeleteOpen(true);
                    }
                    else updateSettings({ deleteArchivesAfter: false });
                  }} />
                  <AdvancedRow icon={<Settings2 />} title="Speed" options={['Balanced', 'Gentle']} value={settings.speed === 'gentle' ? 'Gentle' : 'Balanced'} onChange={(value) => updateSettings({ speed: value === 'Gentle' ? 'gentle' : 'balanced' })} />
                </>
              ) : null}
            </div>
            <footer className="advanced-dialog__footer"><Button onClick={() => setAdvancedOpen(false)}>Done</Button></footer>
          </section>
        </div>
      ) : null}
    </>
  );
}

function PreferenceRow({ icon, title, detail, children }: { icon: ReactNode; title: string; detail?: string; children: ReactNode }) {
  return (
    <div className="preference-row">
      <span className="preference-icon">{icon}</span>
      <div className="preference-copy">
        <strong>{title}</strong>
        {detail ? <small>{detail}</small> : null}
      </div>
      {children}
    </div>
  );
}

function AdvancedRow({
  icon, title, detail, options, value, onChange,
}: {
  icon: ReactNode;
  title: string;
  detail?: string;
  options: readonly string[];
  value: string;
  onChange: (value: string) => void;
}) {
  return (
    <PreferenceRow icon={icon} title={title} detail={detail}>
      <SegmentedControl label={title} options={options} value={value} onChange={onChange} />
    </PreferenceRow>
  );
}

function AdvancedToggle({
  icon, title, checked, onChange,
}: {
  icon: ReactNode;
  title: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}) {
  return <PreferenceRow icon={icon} title={title}><Toggle label={title} checked={checked} onChange={onChange} /></PreferenceRow>;
}
