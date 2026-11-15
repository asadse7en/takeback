import { useMemo, useState, type ReactNode } from 'react';
import { useNavigate } from 'react-router-dom';
import {
  Archive, CalendarDays, Check, Copy, FileText, Folder, FolderOpen,
  Grid2X2, Heart, Image, MapPin, Pencil, Settings2, Star, X,
} from 'lucide-react';
import { Button, PageFrame, Panel, SegmentedControl, Toggle } from '@/components/Ui';
import { mockPreviewMedia, mockSettings } from '@/mock/data';

type SettingsState = {
  albums: string;
  dates: string;
  favorites: string;
  archive: boolean;
  versions: string;
  albumLocation: string;
  editedMode: string;
  trashed: string;
  noJson: string;
  live: boolean;
  splitPhotos: boolean;
  splitVideos: boolean;
  splitLive: boolean;
};

const initialSettings: SettingsState = {
  albums: mockSettings.albums,
  dates: mockSettings.dateFolders,
  favorites: mockSettings.favorites,
  archive: mockSettings.archiveFolder,
  versions: mockSettings.editedPhotos,
  albumLocation: 'Top level',
  editedMode: 'Move',
  trashed: 'Skip',
  noJson: 'Put in Unsorted',
  live: mockSettings.mergeLivePhotos,
  splitPhotos: false,
  splitVideos: false,
  splitLive: false,
};

const folderOptions = ['Off', 'Move', 'Link', 'Copy'] as const;

type PreviewNode = {
  name: string;
  folder: boolean;
  tag?: string;
  children: PreviewNode[];
};

type PreviewGuide = 'empty' | 'vertical' | 'tee' | 'last';
type PreviewLine = Omit<PreviewNode, 'children'> & { guides: PreviewGuide[] };

function createPreviewTree(settings: SettingsState): PreviewLine[] {
  const roots: PreviewNode[] = [];

  const addFile = (folders: string[], name: string, tag?: string) => {
    let children = roots;
    for (const folderName of folders) {
      let folder = children.find((node) => node.folder && node.name === folderName);
      if (!folder) {
        folder = { name: folderName, folder: true, children: [] };
        children.push(folder);
      }
      children = folder.children;
    }
    children.push({ name, folder: false, ...(tag ? { tag } : {}), children: [] });
  };

  for (const media of mockPreviewMedia) {
    if (media.trashed && settings.trashed === 'Skip') continue;
    if (!media.hasJson && settings.noJson === 'Leave in place') continue;

    const isLive = 'live' in media && media.live;
    const dateFolders = () => settings.dates === 'Off'
      ? []
      : settings.dates === 'Year'
        ? [media.year]
        : [media.year, media.month];
    const albumFolders = (name: string) => settings.albumLocation === 'Inside year folders'
      ? [media.year, name]
      : [name];

    let files: { name: string; type: string; editedVersion?: boolean; tag?: string }[];
    if (isLive && settings.live) {
      files = [{ name: `${media.name} + .mov`, type: 'Live', tag: 'Live' }];
    } else if (isLive) {
      files = [
        { name: media.name, type: 'Photos', editedVersion: false },
        { name: media.name.replace(/\.jpg$/i, '.mov'), type: 'Videos', editedVersion: false },
      ];
    } else if (media.edited && settings.versions === 'Edited only') {
      files = [{ name: media.name.replace(/(\.[^.]+)$/, '-edited$1'), type: media.type, editedVersion: true }];
    } else if (media.edited && settings.versions === 'Keep both') {
      files = [
        { name: media.name, type: media.type, editedVersion: false },
        { name: media.name.replace(/(\.[^.]+)$/, '-edited$1'), type: media.type, editedVersion: true },
      ];
    } else {
      files = [{ name: media.name, type: media.type, editedVersion: false }];
    }

    const withTypeFolder = (folders: string[], type: string) => {
      if (type === 'Photos' && settings.splitPhotos) return [...folders, 'Photos'];
      if (type === 'Videos' && settings.splitVideos) return [...folders, 'Videos'];
      if (type === 'Live' && settings.splitLive) return [...folders, 'Live photos'];
      return folders;
    };

    for (const file of files) {
      if (!media.hasJson) {
        addFile(['Unsorted'], file.name, file.tag);
        continue;
      }
      if (media.trashed && settings.trashed === 'Own folder') {
        addFile(['Trash'], file.name, file.tag);
        continue;
      }

      const archiveIsMoved = media.archived && settings.archive;
      const favoriteIsMoved = media.favorite && settings.favorites === 'Move';
      const editedIsMoved = file.editedVersion && settings.editedMode === 'Move';
      let normalFolders = archiveIsMoved
        ? ['Archive']
        : settings.albums === 'Move' && media.album
          ? albumFolders(media.album)
          : dateFolders();
      let mainIsFlat = archiveIsMoved;
      if (favoriteIsMoved) {
        normalFolders = ['Favorites'];
        mainIsFlat = true;
      }
      if (editedIsMoved) {
        normalFolders = ['Edited'];
        mainIsFlat = true;
      }

      if (!mainIsFlat) normalFolders = withTypeFolder(normalFolders, file.type);
      addFile(normalFolders, file.name, file.tag);

      if (media.album && (settings.albums === 'Link' || settings.albums === 'Copy')) {
        addFile(withTypeFolder(albumFolders(media.album), file.type), file.name, settings.albums);
      }

      if (media.favorite && (settings.favorites === 'Link' || settings.favorites === 'Copy')) {
        addFile(['Favorites'], file.name, settings.favorites);
      }

      if (file.editedVersion && (settings.editedMode === 'Link' || settings.editedMode === 'Copy')) {
        addFile(['Edited'], file.name, settings.editedMode);
      }
    }
  }

  const compact = (node: PreviewNode): PreviewNode => {
    let name = node.name;
    let current = node;
    while (current.folder && current.children.length === 1 && current.children[0].folder) {
      current = current.children[0];
      name = `${name} › ${current.name}`;
    }
    return { ...current, name };
  };

  const flatten = (
    nodes: PreviewNode[],
    depth: number,
    ancestorsContinue: boolean[],
  ): PreviewLine[] => {
    const sorted = [...nodes].sort((a, b) => Number(b.folder) - Number(a.folder) || a.name.localeCompare(b.name));
    return sorted.flatMap((rawNode, index): PreviewLine[] => {
      const isLast = index === sorted.length - 1;
      const node = compact(rawNode);
      const guides: PreviewGuide[] = [
        ...ancestorsContinue.map((continues): PreviewGuide => continues ? 'vertical' : 'empty'),
      ];
      if (depth > 0) guides.push(isLast ? 'last' : 'tee');
      const childAncestors = depth === 0
        ? ancestorsContinue
        : [...ancestorsContinue, !isLast];
      return [
        { name: node.name, folder: node.folder, ...(node.tag ? { tag: node.tag } : {}), guides },
        ...flatten(node.children, depth + 1, childAncestors),
      ];
    });
  };

  return flatten([{ name: 'Photo Library', folder: true, children: roots }], 0, []);
}

export default function PreferencesPage() {
  const navigate = useNavigate();
  const [settings, setSettings] = useState(initialSettings);
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const [activeTab, setActiveTab] = useState('Folders');
  const [previewOnly, setPreviewOnly] = useState(false);
  const update = <K extends keyof SettingsState>(key: K, value: SettingsState[K]) =>
    setSettings((current) => ({ ...current, [key]: value }));
  const previewTree = useMemo(() => createPreviewTree(settings), [settings]);

  return (
    <>
      <PageFrame
        className="preferences-page-frame"
        title="Preferences"
        subtitle="Choose how your library is organized."
        footer={
          <>
            <label className="preview-only">
              <input type="checkbox" checked={previewOnly} onChange={(event) => setPreviewOnly(event.target.checked)} />
              Preview only
            </label>
            <div className="footer-buttons">
              <Button onClick={() => navigate('/add')}>Back</Button>
              <Button variant="accent" onClick={() => navigate('/progress')}>Start</Button>
            </div>
          </>
        }
      >
        <div className="preferences-layout">
          <div className="preferences-main">
            <Panel className="output-folder">
              <strong>Output folder</strong>
              <div className="output-path-row">
                <div className="output-path"><Folder size={17} aria-hidden="true" />{mockSettings.outputFolder}</div>
                <Button>Browse</Button>
              </div>
              <div className="disk-meter" aria-label="Disk space: 280 GB used, 131 GB for this export, 101 GB free after">
                <span /><span /><span />
              </div>
              <div className="disk-legend">
                <span>D:</span><span><i className="swatch swatch--used" />280 GB used</span>
                <span><i className="swatch swatch--need" />131 GB this export</span>
                <span><i className="swatch swatch--free" />101 GB free after</span>
              </div>
            </Panel>

            <Panel className="preference-list">
              <PreferenceRow icon={<FolderOpen />} title="Albums" detail="14 albums found">
                <SegmentedControl label="Album handling" options={folderOptions} value={settings.albums} onChange={(value) => update('albums', value)} />
              </PreferenceRow>
              <PreferenceRow icon={<CalendarDays />} title="Date folders">
                <SegmentedControl label="Date folders" options={['Off', 'Year', 'Year › Month']} value={settings.dates} onChange={(value) => update('dates', value)} />
              </PreferenceRow>
              <PreferenceRow icon={<Star />} title="Favorites" detail="1,032 starred photos found">
                <SegmentedControl label="Favorite handling" options={folderOptions} value={settings.favorites} onChange={(value) => update('favorites', value)} />
              </PreferenceRow>
              <PreferenceRow icon={<Pencil />} title="Edited photos" detail="2,140 edited photos found">
                <SegmentedControl label="Edited photos" options={['Keep both', 'Originals only', 'Edited only']} value={settings.versions} onChange={(value) => update('versions', value)} />
              </PreferenceRow>
              <PreferenceRow icon={<Image />} title="Live photos" detail="340 live photos found">
                <span className="preference-control">
                  {settings.live ? <span className="tag">iPhone style</span> : null}
                  <Toggle label="Keep live photos together" checked={settings.live} onChange={(value) => update('live', value)} />
                </span>
              </PreferenceRow>
              <PreferenceRow icon={<Archive />} title="Archive" detail="214 archived photos found">
                <Toggle label="Organize archived photos" checked={settings.archive} onChange={(value) => update('archive', value)} />
              </PreferenceRow>
              <PreferenceRow icon={<Settings2 />} title="Advanced options" detail="All defaults">
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
            </div>
          </Panel>
        </div>
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
                  <AdvancedRow
                    icon={<Folder />}
                    title="Where albums go"
                    options={['Top level', 'Inside year folders']}
                    value={settings.albumLocation}
                    onChange={(value) => update('albumLocation', value)}
                  />
                  <AdvancedRow icon={<Copy />} title="Photo in several albums" options={['First album only', 'Keep in all']} />
                  <AdvancedRow
                    icon={<Pencil />}
                    title="Edited photos folder"
                    detail="Collect edited versions in one folder"
                    options={folderOptions}
                    value={settings.editedMode}
                    onChange={(value) => update('editedMode', value)}
                  />
                  <AdvancedRow
                    icon={<Archive />}
                    title="Trashed items"
                    detail="18 found in your export"
                    options={['Skip', 'Include', 'Own folder']}
                    value={settings.trashed}
                    onChange={(value) => update('trashed', value)}
                  />
                  <PreferenceRow icon={<Grid2X2 />} title="Sort into folders by type" detail="Inside every album and date folder">
                    <div className="checkbox-options">
                      <label><input type="checkbox" checked={settings.splitPhotos} onChange={(event) => update('splitPhotos', event.target.checked)} /> Photos</label>
                      <label><input type="checkbox" checked={settings.splitVideos} onChange={(event) => update('splitVideos', event.target.checked)} /> Videos</label>
                      <label><input type="checkbox" checked={settings.splitLive} onChange={(event) => update('splitLive', event.target.checked)} /> Live</label>
                    </div>
                  </PreferenceRow>
                </>
              ) : null}
              {activeTab === 'Names and duplicates' ? (
                <>
                  <AdvancedRow icon={<FileText />} title="File names" options={['Keep', 'Date and time', 'Date and original']} />
                  <AdvancedRow icon={<Copy />} title="If two files have the same name" options={['Add a number', 'Use date and time', 'Skip']} />
                  <AdvancedToggle icon={<Check />} title="Ignore exact duplicates" checked />
                  <AdvancedRow
                    icon={<Folder />}
                    title="Files without JSON"
                    options={['Leave in place', 'Put in Unsorted']}
                    value={settings.noJson}
                    onChange={(value) => update('noJson', value)}
                  />
                </>
              ) : null}
              {activeTab === 'Dates and metadata' ? (
                <>
                  <AdvancedRow icon={<CalendarDays />} title="Time zone" options={["Photo's local time", "Computer time", 'UTC']} />
                  <AdvancedRow icon={<CalendarDays />} title="If a photo has no date" options={['Use file name', 'Use file date', 'Leave blank']} />
                  <AdvancedToggle icon={<MapPin />} title="Write location" checked />
                  <AdvancedToggle icon={<Pencil />} title="Write descriptions and people tags" checked />
                  <AdvancedToggle icon={<Heart />} title="Rate favorites 5 stars" checked />
                </>
              ) : null}
              {activeTab === 'More' ? (
                <>
                  <AdvancedRow icon={<Image />} title="Live photo style" options={['iPhone', 'Android', 'Separate files']} />
                  <AdvancedToggle icon={<Archive />} title="Delete each zip after it is processed" />
                  <AdvancedRow icon={<Settings2 />} title="Speed" options={['Balanced', 'Gentle']} />
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
  icon, title, detail, options, value: controlledValue, onChange,
}: {
  icon: ReactNode;
  title: string;
  detail?: string;
  options: readonly string[];
  value?: string;
  onChange?: (value: string) => void;
}) {
  const [localValue, setLocalValue] = useState(options[0]);
  const value = controlledValue ?? localValue;
  return (
    <PreferenceRow icon={icon} title={title} detail={detail}>
      <SegmentedControl label={title} options={options} value={value} onChange={onChange ?? setLocalValue} />
    </PreferenceRow>
  );
}

function AdvancedToggle({ icon, title, checked: initialChecked = false }: { icon: ReactNode; title: string; checked?: boolean }) {
  const [checked, setChecked] = useState(initialChecked);
  return <PreferenceRow icon={icon} title={title}><Toggle label={title} checked={checked} onChange={setChecked} /></PreferenceRow>;
}

