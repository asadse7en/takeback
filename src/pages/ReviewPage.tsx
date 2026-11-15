import { useMemo, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { Button, PageFrame, PhotoThumb } from '@/components/Ui';
import { mockReviewItems } from '@/mock/data';

type Filter = 'all' | 'no-json' | 'warnings';
type Choice = 'file-date' | 'pick-date' | 'leave';

const counts: Record<Filter, number> = { all: 170, 'no-json': 128, warnings: 42 };
const filterLabels: Record<Filter, string> = { all: 'All', 'no-json': 'No JSON', warnings: 'Warnings' };

export default function ReviewPage() {
  const navigate = useNavigate();
  const [filter, setFilter] = useState<Filter>('all');
  const [selectedFile, setSelectedFile] = useState<string>(mockReviewItems[0].file);
  const [choice, setChoice] = useState<Choice>('file-date');
  const [pickedDate, setPickedDate] = useState('2022-01-14T12:00');
  const [items, setItems] = useState([...mockReviewItems]);
  const [applySimilar, setApplySimilar] = useState(false);

  const filteredItems = useMemo(
    () => filter === 'all' ? items : items.filter((item) => item.group === filter),
    [filter, items],
  );
  const selected = filteredItems.find((item) => item.file === selectedFile) ?? filteredItems[0];
  const selectedIndex = selected ? filteredItems.findIndex((item) => item.file === selected.file) + 1 : 0;

  const apply = () => {
    if (!selected) return;
    setItems((current) => current.filter((item) => item.file !== selected.file));
    setSelectedFile('');
  };

  return (
    <PageFrame
      title="Review photos"
      subtitle="170 files need a look"
      footer={
        <>
          <span className="footer-note">{selectedIndex || 1} of 170 selected</span>
          <div className="footer-buttons">
            <Button className={applySimilar ? 'is-pressed' : ''} onClick={() => setApplySimilar((value) => !value)}>Apply to all similar</Button>
            <Button variant="accent" onClick={apply} disabled={!selected}>Apply</Button>
          </div>
        </>
      }
    >
      <div className="review-filters" role="group" aria-label="Filter review items">
        {(Object.keys(counts) as Filter[]).map((key) => (
          <button type="button" className={filter === key ? 'is-selected' : ''} aria-pressed={filter === key} key={key} onClick={() => setFilter(key)}>
            {filterLabels[key]} {counts[key]}
          </button>
        ))}
      </div>

      <div className="review-layout">
        <div className="review-grid" aria-label="Review item thumbnails">
          {filteredItems.map((item) => (
            <button type="button" className={`review-item${selected?.file === item.file ? ' is-selected' : ''}`} key={item.file} aria-pressed={selected?.file === item.file} onClick={() => setSelectedFile(item.file)}>
              <PhotoThumb tone={item.tone} />
              <strong>{item.file}</strong>
              <span className={item.group === 'warnings' ? (item.reason === 'Unreadable EXIF' || item.reason === 'Corrupt video' ? 'review-reason is-danger' : 'review-reason') : 'review-reason'}>{item.reason}</span>
            </button>
          ))}
        </div>

        {selected ? (
          <aside className="review-detail" aria-label="Selected item details">
            <PhotoThumb tone={selected.tone} />
            <strong className="review-detail__title">{selected.file}</strong>
            <span className="review-detail__origin">Takeout 001 · Photos from 2022</span>
            <strong className="review-detail__reason">{selected.reason}</strong>
            <p>Google did not export the date and location for this file.</p>
            <fieldset className="review-choices">
              <legend className="visually-hidden">Choose how to handle this item</legend>
              <label>
                <input type="radio" name="review-choice" checked={choice === 'file-date'} onChange={() => setChoice('file-date')} />
                <span><strong>Use the file date</strong><small>Jan 14, 2022 from the photo itself</small></span>
              </label>
              <label>
                <input type="radio" name="review-choice" checked={choice === 'pick-date'} onChange={() => setChoice('pick-date')} />
                <span><strong>Pick a date</strong><small>Choose it yourself</small></span>
              </label>
              {choice === 'pick-date' ? <input className="review-date" type="datetime-local" value={pickedDate} onChange={(event) => setPickedDate(event.target.value)} aria-label="Choose date and time" /> : null}
              <label>
                <input type="radio" name="review-choice" checked={choice === 'leave'} onChange={() => setChoice('leave')} />
                <span><strong>Leave as is</strong><small>Keep the export date</small></span>
              </label>
            </fieldset>
          </aside>
        ) : (
          <aside className="review-detail review-detail--empty">
            <p>No more mock files match this filter.</p>
            <Button onClick={() => navigate('/done')}>Back to summary</Button>
          </aside>
        )}
      </div>
    </PageFrame>
  );
}
