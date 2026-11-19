import { offerSave, releaseSave } from './output.js';

export function element(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text) node.textContent = text;
  return node;
}

// fraction: undefined hides the meter, null shows a busy meter, 0 to 1 fills it.
function setMeter(meter, fraction) {
  meter.hidden = fraction === undefined;
  meter.classList.toggle('busy', fraction === null);
  const value = Math.round((fraction ?? 0) * 100);
  meter.firstChild.style.width = fraction === null ? '' : `${value}%`;
  if (fraction == null) meter.removeAttribute('aria-valuenow');
  else meter.setAttribute('aria-valuenow', value);
}

// The Downloads list: one row per song or album, newest first.
export function createJobs(section, clearButton) {
  const list = section.querySelector('ul');

  function update() {
    const items = [...list.children];
    section.hidden = !items.length;
    clearButton.disabled = items.every((item) => item.dataset.active);
  }

  clearButton.addEventListener('click', () => {
    for (const item of [...list.children]) {
      if (item.dataset.active) continue;
      releaseSave(item.querySelector('.save'));
      item.remove();
    }
    update();
  });

  return function addJob({ name, path, onCancel }) {
    const item = element('li');
    item.dataset.active = 'true';
    const text = element('div', 'file');
    const label = element('span', 'file-name', name);
    label.title = path;
    const detail = element('span', 'file-details');
    const meter = element('div', 'meter');
    meter.setAttribute('role', 'progressbar');
    meter.setAttribute('aria-label', `${name} progress`);
    meter.append(element('span'));
    text.append(label, detail, meter);
    const cancel = element('button', '', 'Cancel');
    cancel.type = 'button';
    cancel.setAttribute('aria-label', `Cancel ${name}`);
    cancel.addEventListener('click', onCancel);
    const save = element('a', 'save', 'Save');
    save.hidden = true;
    item.append(text, cancel, save);
    list.prepend(item);
    update();
    return {
      meter,
      show({ text: message, fraction }, stage = '') {
        detail.textContent = message;
        detail.className = `file-details ${stage}`.trim();
        setMeter(meter, fraction);
      },
      offer(blob, fileName, linkText = 'Save') {
        save.textContent = linkText;
        save.setAttribute('aria-label', `Save ${fileName}`);
        offerSave(save, blob, fileName);
      },
      finish() {
        cancel.remove();
        delete item.dataset.active;
        update();
      },
    };
  };
}
