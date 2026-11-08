! Shapes, a module of geometry and a program that uses it.
#define NPOINTS 4
module shapes
  use, intrinsic :: iso_fortran_env, only: real64, int32
  implicit none
  private
  public :: point, circle, area, scale_points

  real(real64), parameter :: pi = 3.141592653589793_real64
  integer(int32), parameter :: mask = int(z'ff', int32)

  type :: point
    real(real64) :: x = 0.0d0, y = 0.0d0
  end type point

  type, extends(point) :: circle
    real(real64) :: radius = 1.0_real64
  contains
    procedure :: area => circle_area
  end type circle

  interface area
    module procedure circle_area
  end interface area

contains

  pure function circle_area(self) result(a)
    class(circle), intent(in) :: self
    real(real64) :: a
    a = pi * self%radius**2
  end function circle_area

  elemental subroutine scale_points(p, factor)
    type(point), intent(inout) :: p
    real(real64), intent(in) :: factor
    p%x = p%x * factor
    p%y = p%y * factor
  end subroutine scale_points

end module shapes

program main
  use shapes
  implicit none
  type(circle) :: c
  type(point), allocatable :: pts(:)
  character(len=*), parameter :: title = 'Bob''s "circles"'
  character(len=32) :: label
  integer :: i, unit, status
  logical :: verbose = .true.
  complex :: z = (1.0, -2.5e-3)

  allocate(pts(NPOINTS))
  c = circle(x=1.0d0, y=2.0d0, radius=3.5d0)
  call scale_points(pts, 2.0d0)

  outer: do i = 1, size(pts)
    if (i > 3 .and. .not. verbose) then
      exit outer
    else if (mod(i, 2) == 0) then
      cycle
    else
      pts(i)%x = real(i, real64) / 2
    end if
  end do outer

  select case (i)
  case (1:3)
    label = "few"
  case default
    label = "many ""points"""
  end select

  do while (status /= 0)
    status = status - 1
  end do

  where (pts%x > 1.0d0) pts%y = 0.0d0

  block
    integer :: n
    read(*, *, iostat=status) n
    if (status /= 0) error stop "bad input"
  end block

  open(newunit=unit, file='out.txt', status='replace', iostat=status)
  write(unit, '(a, f8.3)') title, area(c)
  close(unit)

  print *, trim(label), abs(z), .false.
  deallocate(pts)
  stop 0
end program main
