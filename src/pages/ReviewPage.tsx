import { useEffect, useMemo, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { Button, PageFrame, PhotoThumb } from '@/components/Ui';
import { callCommand, type ReportItem } from '@/lib/api';
import { useReportStore } from '@/store/report';

type Filter = 'all' | 'no-json' | 'warnings';
type Choice = 'fileDate' | 'pickDate' | 'leave';

const filterLabels: Record<Filter, string> = { all: 'All', 'no-json': 'No JSON', warnings: 'Warnings' };

export default function ReviewPage() {
  const navigate = useNavigate();
  const setReport = useReportStore((state) => state.setReport);
  const [filter, setFilter] = useState<Filter>('all');
  const [selectedId, setSelectedId] = useState('');
  const [choice, setChoice] = useState<Choice>('fileDate');
  const [pickedDate, setPickedDate] = useState('');
  const [items, setItems] = useState<ReportItem[]>([]);
  const [applySimilar, setApplySimilar] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');

  useEffect(() => {
    callCommand('get_review_items', {}).then((reviewItems) => {
      setItems(reviewItems);
      setSelectedId(reviewItems[0]?.id ?? '');
    }).catch((reason: unknown) => {
      setError(reason instanceof Error ? reason.message : String(reason));
    });
  }, []);

  const filteredItems = useMemo(() => {
    if (filter === 'all') return items;
    if (filter === 'no-json') return items.filter((item) => item.group === 'no-json');
    return items.filter((item) => item.group !== 'no-json');
  }, [filter, items]);
  const selected = filteredItems.find((item) => item.id === selectedId) ?? filteredItems[0];
  const counts: Record<Filter, number> = {
    all: items.length,
    'no-json': items.filter((item) => item.group === 'no-json').length,
    warnings: items.filter((item) => item.group !== 'no-json').length,
  };
  const selectedIndex = selected ? filteredItems.findIndex((item) => item.id === selected.id) + 1 : 0;

  const apply = async () => {
    if (!selected) return;
    setBusy(true);
    setError('');
    const targets = applySimilar
      ? filteredItems.filter((item) => item.reason === selected.reason && item.group === selected.group)
      : [selected];
    try {
      for (const item of targets) {
        await callCommand('apply_review_fix', {
          itemId: item.id,
          choice,
          pickedDate: choice === 'pickDate' ? pickedDate : null,
        });
        setItems((current) => current.filter((entry) => entry.id !== item.id));
      }
      const report = await callCommand('get_report', {});
      setReport(report);
      const remaining = items.filter((item) => !targets.some((target) => target.id === item.id));
      setSelectedId(remaining[0]?.id ?? '');
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
      try {
        const [reviewItems, report] = await Promise.all([
          callCommand('get_review_items', {}),
          callCommand('get_report', {}),
        ]);
        setItems(reviewItems);
        setReport(report);
      } catch {
        setError((current) => `${current} The report could not be refreshed.`);
      }
    } finally {
      setBusy(false);
    }
  };

  return (
    <PageFrame
      title="Review photos"
      subtitle={`${items.length.toLocaleString()} files need a look`}
      footer={
        <>
          <span className="footer-note">{selectedIndex || 0} of {filteredItems.length.toLocaleString()} selected</span>
          <div className="footer-buttons">
            <Button className={applySimilar ? 'is-pressed' : ''} disabled={!selected} onClick={() => setApplySimilar((value) => !value)}>Apply to all similar</Button>
            <Button variant="accent" disabled={!selected || busy || (choice === 'pickDate' && !pickedDate)} onClick={() => void apply()}>{busy ? 'Applying…' : 'Apply'}</Button>
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
          {filteredItems.map((item, index) => (
            <button type="button" className={`review-item${selected?.id === item.id ? ' is-selected' : ''}`} key={item.id} aria-pressed={selected?.id === item.id} onClick={() => setSelectedId(item.id)}>
              <PhotoThumb tone={index % 2 ? 'blue' : 'peach'} />
              <strong>{item.file}</strong>
              <span className={item.group === 'failed' ? 'review-reason is-danger' : 'review-reason'}>{item.reason}</span>
            </button>
          ))}
        </div>

        {selected ? (
          <aside className="review-detail" aria-label="Selected item details">
            <PhotoThumb tone="peach" />
            <strong className="review-detail__title">{selected.file}</strong>
            <span className="review-detail__origin">{selected.destination ?? selected.group}</span>
            <strong className="review-detail__reason">{selected.reason}</strong>
            <p>Choose how Takeback should handle this output file.</p>
            <fieldset className="review-choices">
              <legend className="visually-hidden">Choose how to handle this item</legend>
              <label>
                <input type="radio" name="review-choice" checked={choice === 'fileDate'} onChange={() => setChoice('fileDate')} />
                <span><strong>Use the file date</strong><small>Use the date already stored in the photo</small></span>
              </label>
              <label>
                <input type="radio" name="review-choice" checked={choice === 'pickDate'} onChange={() => setChoice('pickDate')} />
                <span><strong>Pick a date</strong><small>Choose it yourself</small></span>
              </label>
              {choice === 'pickDate' ? <input className="review-date" type="datetime-local" value={pickedDate} onChange={(event) => setPickedDate(event.target.value)} aria-label="Choose date and time" /> : null}
              <label>
                <input type="radio" name="review-choice" checked={choice === 'leave'} onChange={() => setChoice('leave')} />
                <span><strong>Leave as is</strong><small>Keep the current output unchanged</small></span>
              </label>
            </fieldset>
          </aside>
        ) : (
          <aside className="review-detail review-detail--empty">
            <p>No review items match this filter.</p>
            <Button onClick={() => navigate('/done')}>Back to summary</Button>
          </aside>
        )}
      </div>
      {error ? <p role="alert" className="form-error">{error}</p> : null}
    </PageFrame>
  );
}
