class SubmittedChangeSet {
  clientId: string = '';
  ops: Array<DeleteOp | InsertOp | RetainOp> = [];
}

class DeleteOp {
  count: number = 0;
}

class InsertOp {
  content: string = '';
}

class RetainOp {
  count: number = 0;
}

export default {
  DeleteOp: DeleteOp,
  InsertOp: InsertOp,
  RetainOp: RetainOp,

  SubmittedChangeSet: SubmittedChangeSet,
};
