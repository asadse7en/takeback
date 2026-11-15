export const mockArchives = [
  {
    name: 'takeout-20260612T101500Z-001.zip',
    size: '49.8 GB',
    status: 'Indexed',
  },
  {
    name: 'takeout-20260612T101500Z-002.zip',
    size: '49.9 GB',
    status: 'Indexed',
  },
  {
    name: 'takeout-20260612T101500Z-003.zip',
    size: '31.4 GB',
    status: 'Indexed',
  },
] as const;

export const mockCounts = {
  photos: 12480,
  videos: 1230,
  livePhotos: 340,
  total: '131 GB',
} as const;

export const mockSettings = {
  outputFolder: '<Pictures>/Photo Library',
  albums: 'Move',
  dateFolders: 'Year › Month',
  favorites: 'Link',
  archiveFolder: true,
  mergeLivePhotos: true,
  editedPhotos: 'Keep both',
} as const;

export const mockProgress = {
  completedFiles: 8870,
  totalFiles: 13710,
  percent: 64,
  currentFile: 'IMG_4821.HEIC',
  filesPerSecond: 412,
  minutesRemaining: 6,
  warnings: 42,
  recentFiles: [
    { name: 'IMG_4820.HEIC', status: 'Fixed', tone: 'peach' },
    { name: 'PXL_20240612.jpg', status: 'Live paired', tone: 'blue' },
    { name: 'VID_0099.mp4', status: 'No JSON', tone: 'green' },
    { name: 'DSC_0231.jpg', status: 'Fixed', tone: 'rose' },
    { name: 'IMG_4818.HEIC', status: 'Fixed', tone: 'blue' },
  ],
} as const;

export const mockCompletion = {
  filesFixed: 12310,
  filesChecked: 13710,
  duplicates: 1230,
  noJson: 128,
  warnings: 42,
  datesRestored: 12198,
  locationsAdded: 9402,
  livePhotosMerged: 338,
  saved: '128 GB saved in 31 min',
} as const;

export const mockReviewItems = [
  { file: 'IMG_0099.jpg', reason: 'No JSON found', tone: 'rose', group: 'no-json' },
  { file: 'VID_0099.mp4', reason: 'No JSON found', tone: 'green', group: 'no-json' },
  { file: 'DSC_0231.jpg', reason: 'Unreadable EXIF', tone: 'peach', group: 'warnings' },
  { file: 'IMG_0412.heic', reason: 'No JSON found', tone: 'blue', group: 'no-json' },
  { file: 'PXL_0188.mp4', reason: 'Corrupt video', tone: 'green', group: 'warnings' },
  { file: 'IMG_1204.jpg', reason: 'Name clash', tone: 'rose', group: 'warnings' },
  { file: 'IMG_0377.jpg', reason: 'No JSON found', tone: 'peach', group: 'no-json' },
  { file: 'IMG_0520.jpg', reason: 'No JSON found', tone: 'blue', group: 'no-json' },
] as const;

export const mockReportRows = [
  ['Folders', 'Albums first, then Year › Month'],
  ['Albums', 'Move'],
  ['Favorites', 'Link'],
  ['Archive', 'Own folder'],
  ['Time zone', "Photo's local time"],
  ['Live photos', 'Separate files'],
  ['Edited versions', 'Keep both'],
] as const;

export const mockReportItems = [
  { file: 'IMG_0099.jpg', issue: 'No JSON found', result: 'Date kept', tone: 'rose' },
  { file: 'VID_0099.mp4', issue: 'No JSON found', result: 'Date kept', tone: 'green' },
  { file: 'DSC_0231.jpg', issue: 'Unreadable EXIF', result: 'Metadata skipped', tone: 'peach' },
  { file: 'IMG_0412.heic', issue: 'No JSON found', result: 'Date kept', tone: 'blue' },
  { file: 'PXL_0188.mp4', issue: 'Corrupt video', result: 'Copied as is', tone: 'green' },
  { file: 'IMG_1204.jpg', issue: 'Name clash', result: 'Renamed', tone: 'rose' },
] as const;

export const mockPreviewMedia = [
  {
    name: 'IMG_0042.jpg',
    year: '2022',
    month: 'January',
    album: 'Summer trip',
    type: 'Photos',
    favorite: true,
    archived: false,
    edited: true,
    hasJson: true,
    trashed: false,
  },
  {
    name: 'VID_0012.mp4',
    year: '2022',
    month: 'January',
    album: null,
    type: 'Videos',
    favorite: false,
    archived: false,
    edited: false,
    hasJson: true,
    trashed: false,
  },
  {
    name: 'IMG_1045.jpg',
    year: '2023',
    month: 'July',
    album: 'Summer trip',
    type: 'Photos',
    favorite: false,
    archived: false,
    edited: false,
    hasJson: true,
    trashed: false,
  },
  {
    name: 'IMG_0840.jpg',
    year: '2022',
    month: 'March',
    album: 'Family',
    type: 'Photos',
    favorite: true,
    archived: true,
    edited: false,
    hasJson: true,
    trashed: false,
  },
  {
    name: 'IMG_0542.jpg',
    year: '2023',
    month: 'June',
    album: 'Family',
    type: 'Live',
    favorite: false,
    archived: false,
    edited: false,
    live: true,
    hasJson: true,
    trashed: false,
  },
  {
    name: 'IMG_3901.jpg',
    year: '2023',
    month: 'July',
    album: 'Summer trip',
    type: 'Photos',
    favorite: false,
    archived: false,
    edited: false,
    hasJson: true,
    trashed: false,
  },
  {
    name: 'IMG_0201.jpg',
    year: '2024',
    month: 'April',
    album: null,
    type: 'Photos',
    favorite: false,
    archived: false,
    edited: false,
    hasJson: true,
    trashed: true,
  },
  {
    name: 'IMG_0099.jpg',
    year: '2022',
    month: 'February',
    album: null,
    type: 'Photos',
    favorite: false,
    archived: false,
    edited: false,
    hasJson: false,
    trashed: false,
  },
] as const;
