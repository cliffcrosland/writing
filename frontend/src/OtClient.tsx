import React, { useState } from 'react';
import './OtClient.css';

class Selection {
  start: number = 0;
  end: number = 0;
}

class DeleteOp {
  readonly count: number;

  constructor(count: number) {
    this.count = count;
  }

  toString(): string {
    return `Delete(${this.count})`;
  }
}

class InsertOp {
  readonly content: string;

  constructor(content: string) {
    this.content = content;
  }

  toString(): string {
    return `Insert("${this.content}")`;
  }
}

class RetainOp {
  readonly count: number;

  constructor(count: number) {
    this.count = count;
  }

  toString(): string {
    return `Retain(${this.count})`;
  }
}

class ChangeSet {
  ops: Array<DeleteOp | InsertOp | RetainOp>;

  constructor() {
    this.ops = new Array<Op>();
  }

  delete(count: number) {
    if (count <= 0) return;
    this.ops.push(new DeleteOp(count));
  }

  insert(content: string) {
    if (content.length === 0) return;
    this.ops.push(new InsertOp(content));
  }

  retain(count: number) {
    if (count <= 0) return;
    this.ops.push(new RetainOp(count));
  }

  toString(): string {
    return this.ops.map((op) => op.toString()).join(', ');
  }
}

function OtClient(props: any) {
  const clientId: string = props.clientId;

  const initSelection: Selection = { start: 0, end: 0 };
  const [changeSets, setChangeSets] = useState(new Array<ChangeSet>());
  const [selection, setSelection] = useState(initSelection);
  const [value, setValue] = useState('');
  
  function captureSelection(event) {
    // Capture selection before the input event (i.e. deletion, insertion).
    const newSelection: Selection = {
      start: event.target.selectionStart,
      end: event.target.selectionEnd,
    };
    console.log(newSelection);
    setSelection(newSelection);
  }

  function onInput(event) {
    const newSelection: Selection = {
      start: event.target.selectionStart,
      end: event.target.selectionEnd,
    };
    console.log();
    console.log('onInput');
    console.log('inputType: ' + event.nativeEvent.inputType);
    console.log('data: ' + event.nativeEvent.data);
    console.log('target value: ' + event.target.value);
    console.log('prev selection:');
    console.log(selection);
    console.log('new selection:');
    console.log(newSelection);

    console.log('prev value length: ' + value.length);
    console.log('new value length: ' + event.target.value.length);

    const newValue = event.target.value;
    const inputType: string = event.nativeEvent.inputType;
    const changeSet: ChangeSet = new ChangeSet();
    const selectionLength = selection.end - selection.start;
    switch (inputType) {
      case 'deleteByCut':
      case 'deleteByDrag':
        changeSet.retain(selection.start);
        changeSet.delete(selectionLength);
        changeSet.retain(value.length - selection.end);
        break;
      case 'deleteContentBackward':
      case 'deleteContentForward':
        if (selectionLength > 0) {
          changeSet.retain(selection.start);
          changeSet.delete(selectionLength);
          changeSet.retain(newValue.length - newSelection.end);
        } else {
          const deletedCount = value.length - newValue.length;
          changeSet.retain(newSelection.start);
          changeSet.delete(deletedCount);
          changeSet.retain(newValue.length - newSelection.end);
        }
        break;
      case 'insertFromDrop':
        changeSet.retain(newSelection.start - event.nativeEvent.data.length);
        changeSet.insert(event.nativeEvent.data);
        changeSet.retain(newValue.length - newSelection.end);
        break;
      case 'insertFromPaste':
      case 'insertText':
        changeSet.retain(selection.start);
        changeSet.delete(selectionLength);
        changeSet.insert(event.nativeEvent.data)
        changeSet.retain(value.length - selection.end);
        break;
    }
    console.log('changeSet:');
    console.log(changeSet.toString());
    setChangeSets([...changeSets, changeSet]);
    setSelection(newSelection);
    setValue(newValue);
  }

  return (
    <div className="OtClient">
      <h2>Client ID: {clientId}</h2>
      <textarea 
        className="OtClient-text"
        onDragStart={captureSelection}
        onKeyDown={captureSelection}
        onSelect={captureSelection}
        onInput={onInput}
      ></textarea>
      <div className="OtClient-changeSets">
        {changeSets.map((changeSet, index) =>
          <div key={index}>{changeSet.toString()}</div>
        ).reverse()}
      </div>
    </div>
  );
}

export default OtClient;
