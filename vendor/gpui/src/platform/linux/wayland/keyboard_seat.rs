/// Keep the active keyboard while possible, otherwise choose a capable seat, not the last global.
pub(super) fn select<'a, T: PartialEq>(
    seats: &'a [(u32, T, bool)],
    active: Option<&T>,
) -> Option<&'a T> {
    let mut keyboards = seats
        .iter()
        .filter(|(_, _, capable)| *capable)
        .map(|(_, seat, _)| seat);
    keyboards
        .clone()
        .find(|seat| Some(*seat) == active)
        .or_else(|| keyboards.next())
}
