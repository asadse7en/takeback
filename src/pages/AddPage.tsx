import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { Database, Disc3, FileText, Image, Upload, Video, X } from 'lucide-react';
import { Button, PageFrame, PageHeading, Panel, StatusCheck } from '@/components/Ui';
import { mockArchives, mockCounts } from '@/mock/data';

export default function AddPage() {
  const navigate = useNavigate();
  const [archives, setArchives] = useState([...mockArchives]);
  const stats = [
    { label: 'Photos', value: mockCounts.photos.toLocaleString(), Icon: Image },
    { label: 'Videos', value: mockCounts.videos.toLocaleString(), Icon: Video },
    { label: 'Live photos', value: mockCounts.livePhotos.toLocaleString(), Icon: Disc3 },
    { label: 'Total', value: mockCounts.total, Icon: Database },
  ];

  return (
    <PageFrame
      title="Add your Takeout files"
      subtitle="Add every zip from your Google Photos export."
      footer={<><span /><Button variant="accent" onClick={() => navigate('/preferences')}>Next</Button></>}
    >
      <button className="drop-zone" type="button" aria-label="Browse for Takeout files">
        <Upload size={30} strokeWidth={1.5} aria-hidden="true" />
        <strong>Drop your Takeout here</strong>
        <span className="drop-zone__button">Browse</span>
      </button>

      <PageHeading>Added files ({archives.length})</PageHeading>
      <Panel className="archive-list">
        {archives.map((archive) => (
          <div className="archive-row" key={archive.name}>
            <FileText className="archive-row__file" size={20} strokeWidth={1.5} aria-hidden="true" />
            <span className="archive-row__name">{archive.name}</span>
            <span className="archive-row__size">{archive.size}</span>
            <StatusCheck>{archive.status}</StatusCheck>
            <button
              type="button"
              className="icon-button"
              aria-label={`Remove ${archive.name}`}
              onClick={() => setArchives((current) => current.filter((item) => item.name !== archive.name))}
            >
              <X size={15} aria-hidden="true" />
            </button>
          </div>
        ))}
      </Panel>

      <Panel className="archive-stats">
        {stats.map(({ label, value, Icon }, index) => (
          <div className="archive-stat" key={label}>
            {index > 0 ? <span className="archive-stat__divider" aria-hidden="true" /> : null}
            <span className="archive-stat__icon"><Icon size={20} strokeWidth={1.5} aria-hidden="true" /></span>
            <span><strong>{value}</strong><small>{label}</small></span>
          </div>
        ))}
      </Panel>
    </PageFrame>
  );
}
